//! Read-only validation of the small, safe subset of chart-to-XLSX mappings.
//!
//! This deliberately produces no writable handles. It accepts only contiguous A1 ranges on
//! ordinary worksheets and verifies every displayed cache value against the backing cell.
use std::collections::{BTreeMap, BTreeSet};

use crate::xml::{self, El};
use crate::{
    DocxError,
    package::{ContentTypes, EMBEDDED_PARTS, EmbeddedManifest, Package, Rel, rel_is, zip_entries},
    preserved_xml::{self, Element, qualified},
};
use sha2::{Digest, Sha256};

const MAX_POINTS: usize = 10_000;
const MAX_SERIES: usize = 256;
const MAX_MAPPED_CELLS: usize = 100_000;
const MAX_LABEL_BYTES: usize = 4096;
const XLSX_OFFICE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const DRAWING_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Classification {
    Mappable(Box<ChartMapping>),
    Unsupported(String),
    Ambiguous(String),
    Inconsistent(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ChartMapping {
    pub chart_kind: String,
    pub workbook_part: String,
    pub legend_position: Option<String>,
    pub chart_title: Option<String>,
    pub horizontal_axis_title: Option<String>,
    pub vertical_axis_title: Option<String>,
    pub horizontal_axis: Option<ChartAxisSnapshot>,
    pub vertical_axis: Option<ChartAxisSnapshot>,
    pub series: Vec<SeriesMapping>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartAxisSnapshot {
    /// `value`, `scatterX`, `scatterY`, or `category` (deliberately read-only).
    pub role: String,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub number_format: Option<String>,
    pub editable: bool,
    pub number_format_editable: bool,
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase", deny_unknown_fields)]
pub enum AxisBoundChange {
    Auto,
    Manual { value: f64 },
}

#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AxisChanges {
    pub minimum: Option<AxisBoundChange>,
    pub maximum: Option<AxisBoundChange>,
    pub number_format: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SeriesMapping {
    pub name: MappedCell<String>,
    pub data: SeriesDataMapping,
    pub formatting: SeriesFormatSnapshot,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesFormatSnapshot {
    pub color: Option<String>,
    pub color_editable: bool,
    pub line_width: Option<f64>,
    pub line_width_editable: bool,
    pub marker_symbol: Option<String>,
    pub marker_symbol_editable: bool,
    pub marker_size: Option<u8>,
    pub marker_size_editable: bool,
    pub marker_color: Option<String>,
    pub marker_color_editable: bool,
    #[serde(default)]
    pub data_labels: Option<DataLabelSnapshot>,
    #[serde(default)]
    pub error_bars: Vec<ErrorBarSnapshot>,
    pub diagnostic: Option<String>,
}

/// Read-only error-bar summary. Values are intentionally not editable in this phase.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBarSnapshot {
    pub direction: String,
    pub bar_type: Option<String>,
    pub value_type: Option<String>,
    pub renderable: bool,
    pub diagnostic: Option<String>,
}

/// Editable only when the source has one plain per-series `c:dLbls` block with explicit
/// attributes. We deliberately do not synthesize label XML or rewrite point-level/custom labels.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataLabelSnapshot {
    pub show_value: Option<bool>,
    pub show_category: Option<bool>,
    pub show_series_name: Option<bool>,
    pub position: Option<String>,
    pub editable: bool,
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DataLabelChanges {
    pub show_value: Option<bool>,
    pub show_category: Option<bool>,
    pub show_series_name: Option<bool>,
    pub position: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SeriesDataMapping {
    CategoryValues { categories: Vec<MappedCell<String>>, values: Vec<MappedCell<f64>> },
    Scatter { x_values: Vec<MappedCell<f64>>, y_values: Vec<MappedCell<f64>> },
}

#[allow(dead_code)] // Convenient read-only views are also exercised directly by mapper tests.
impl SeriesMapping {
    fn categories(&self) -> &[MappedCell<String>] {
        match &self.data {
            SeriesDataMapping::CategoryValues { categories, .. } => categories,
            SeriesDataMapping::Scatter { .. } => &[],
        }
    }

    fn x_values(&self) -> &[MappedCell<f64>] {
        match &self.data {
            SeriesDataMapping::CategoryValues { .. } => &[],
            SeriesDataMapping::Scatter { x_values, .. } => x_values,
        }
    }

    fn values(&self) -> &[MappedCell<f64>] {
        match &self.data {
            SeriesDataMapping::CategoryValues { values, .. } => values,
            SeriesDataMapping::Scatter { y_values, .. } => y_values,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MappedCell<T> {
    pub worksheet: String,
    pub worksheet_part: String,
    pub address: String,
    pub value: T,
    source: CellSource,
}

#[derive(Clone, Debug, PartialEq)]
enum CellSource {
    SharedString(usize),
    InlineString,
    StringValue,
    Number,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CellAddress {
    col: u32,
    row: u32,
}

#[derive(Clone, Debug)]
struct Range {
    sheet: String,
    cells: Vec<CellAddress>,
}

#[derive(Clone, Debug)]
enum CellValue {
    Text(String, CellSource),
    Number(f64),
    Unsupported(String),
}

/// Validate one chart part and its relationship graph against an embedded XLSX package.
/// This API is read-only: it receives immutable part bytes and returns value copies only.
pub(crate) fn map_chart(chart_xml: &[u8], chart_rels: &[Rel], workbook_part: &str, workbook: &[u8]) -> Classification {
    match map_chart_inner(chart_xml, chart_rels, workbook_part, workbook) {
        Ok(mapping) => Classification::Mappable(Box::new(mapping)),
        Err(Reason::Unsupported(s)) => Classification::Unsupported(s),
        Err(Reason::Ambiguous(s)) => Classification::Ambiguous(s),
        Err(Reason::Inconsistent(s)) => Classification::Inconsistent(s),
    }
}

#[derive(Debug)]
enum Reason {
    Unsupported(String),
    Ambiguous(String),
    Inconsistent(String),
}
type MResult<T> = Result<T, Reason>;

struct ChartTitleNodes {
    nodes: Vec<Element>,
    chart: Option<usize>,
    horizontal_axis: Option<usize>,
    vertical_axis: Option<usize>,
}

fn inspect_chart_title_nodes(chart_xml: &[u8], chart_kind: &str) -> MResult<ChartTitleNodes> {
    let source = preserved_xml::utf8(chart_xml).map_err(|_| unsupported("chart title XML is malformed"))?;
    let nodes = preserved_xml::inspect(source.as_ref()).map_err(|_| unsupported("chart title XML is malformed"))?;
    if nodes.is_empty() || !nodes.first().is_some_and(|root| root.is(CHART_NS, "chartSpace")) {
        return Err(unsupported("chart title XML root is invalid"));
    }
    let chart_nodes = child_indices(&nodes, 0, CHART_NS, "chart");
    let chart_node = (chart_nodes.len() == 1).then(|| chart_nodes[0]);
    let chart_title = chart_node.and_then(|chart| {
        let titles = child_indices(&nodes, chart, CHART_NS, "title");
        (titles.len() == 1).then(|| titles[0])
    });
    let mut horizontal_axis = None;
    let mut vertical_axis = None;
    if !matches!(chart_kind, "pie" | "doughnut")
        && let Some(chart_node) = chart_node
    {
        let plot_nodes = child_indices(&nodes, chart_node, CHART_NS, "plotArea");
        if plot_nodes.len() == 1 {
            let plot = plot_nodes[0];
            let chart_types = child_indices_any(&nodes, plot, CHART_NS, &["barChart", "lineChart", "areaChart", "scatterChart"]);
            if chart_types.len() == 1 {
                let chart_type = chart_types[0];
                let chart_axis_nodes = child_indices(&nodes, chart_type, CHART_NS, "axId");
                let chart_axis_ids: Vec<_> =
                    chart_axis_nodes.iter().filter_map(|index| nodes.get(*index)?.attrs.get(&qualified("", "val")).cloned()).collect();
                let unique_axis_ids: BTreeSet<_> = chart_axis_ids.iter().collect();
                // Secondary or malformed axis sets can reuse IDs. Do not guess which title
                // the user intended when references are incomplete or ambiguous.
                let axis_references_are_unambiguous =
                    chart_axis_ids.len() == chart_axis_nodes.len() && chart_axis_ids.len() == unique_axis_ids.len() && chart_axis_ids.len() == 2;
                let axes: Vec<_> = child_indices_any(&nodes, plot, CHART_NS, &["catAx", "dateAx", "valAx"])
                    .into_iter()
                    .filter_map(|axis| {
                        let axis_ids = child_indices(&nodes, axis, CHART_NS, "axId");
                        let axis_id = (axis_ids.len() == 1).then(|| nodes.get(axis_ids[0])?.attrs.get(&qualified("", "val")).cloned())??;
                        if !axis_references_are_unambiguous || !chart_axis_ids.contains(&axis_id) {
                            return None;
                        }
                        let positions = child_indices(&nodes, axis, CHART_NS, "axPos");
                        let position = (positions.len() == 1).then(|| nodes.get(positions[0])?.attrs.get(&qualified("", "val")).cloned())??;
                        Some((axis, position))
                    })
                    .collect();
                let horizontal: Vec<_> = axes.iter().filter(|(_, position)| matches!(position.as_str(), "b" | "t")).map(|(axis, _)| *axis).collect();
                let vertical: Vec<_> = axes.iter().filter(|(_, position)| matches!(position.as_str(), "l" | "r")).map(|(axis, _)| *axis).collect();
                if horizontal.len() == 1 {
                    let titles = child_indices(&nodes, horizontal[0], CHART_NS, "title");
                    horizontal_axis = (titles.len() == 1).then(|| titles[0]);
                }
                if vertical.len() == 1 {
                    let titles = child_indices(&nodes, vertical[0], CHART_NS, "title");
                    vertical_axis = (titles.len() == 1).then(|| titles[0]);
                }
            }
        }
    }
    Ok(ChartTitleNodes { nodes, chart: chart_title, horizontal_axis, vertical_axis })
}

fn editable_title_text(nodes: &[Element], title: usize) -> Option<(usize, String)> {
    let tx = child_indices(nodes, title, CHART_NS, "tx");
    if tx.len() != 1 {
        return None;
    }
    let rich = child_indices(nodes, tx[0], CHART_NS, "rich");
    if rich.len() != 1 || !child_indices(nodes, tx[0], CHART_NS, "strRef").is_empty() {
        return None;
    }
    let paragraphs = descendants(nodes, rich[0], DRAWING_NS, "p");
    let runs = descendants(nodes, rich[0], DRAWING_NS, "r");
    let texts = descendants(nodes, rich[0], DRAWING_NS, "t");
    if paragraphs.len() != 1 || runs.len() != 1 || texts.len() != 1 {
        return None;
    }
    let paragraph = *paragraphs.first()?;
    let run = *runs.first()?;
    let text = *texts.first()?;
    if !child_indices(nodes, paragraph, DRAWING_NS, "r").contains(&run)
        || !child_indices(nodes, run, DRAWING_NS, "t").contains(&text)
        || nodes.get(text)?.empty
        || nodes.get(text)?.text.trim().is_empty()
    {
        return None;
    }
    Some((text, nodes.get(text)?.text.clone()))
}

fn unsupported(s: &str) -> Reason {
    Reason::Unsupported(s.to_string())
}
fn ambiguous(s: &str) -> Reason {
    Reason::Ambiguous(s.to_string())
}
fn inconsistent(s: &str) -> Reason {
    Reason::Inconsistent(s.to_string())
}

fn map_chart_inner(chart_xml: &[u8], chart_rels: &[Rel], workbook_part: &str, workbook: &[u8]) -> MResult<ChartMapping> {
    let chart = xml::parse(chart_xml).map_err(|_| unsupported("chart XML is malformed"))?;
    if chart.name != "c:chartSpace" || contains_local(&chart, "extLst", 0) {
        return Err(unsupported("chart root or extension markup is unsupported"));
    }
    if chart_rels.iter().any(|r| r.external) {
        return Err(unsupported("chart has an external relationship"));
    }
    let books: Vec<_> = chart_rels.iter().filter(|r| rel_is(&r.kind, &format!("{XLSX_OFFICE}package"))).collect();
    if books.len() != 1 {
        return Err(if books.is_empty() {
            Reason::Unsupported("chart has no embedded workbook relationship".into())
        } else {
            ambiguous("chart has multiple embedded workbook relationships")
        });
    }
    if books[0].target != workbook_part {
        return Err(inconsistent("resolved workbook does not match the chart relationship"));
    }
    let external_data_id =
        chart.find("c:externalData").and_then(|e| e.attr("r:id")).ok_or_else(|| unsupported("embedded workbook reference is missing"))?;
    if books[0].id != external_data_id {
        return Err(inconsistent("chart externalData reference does not match its workbook relationship"));
    }
    if chart_rels.iter().any(|r| !rel_is(&r.kind, &format!("{XLSX_OFFICE}package"))) {
        return Err(unsupported("chart has additional relationship types outside the supported subset"));
    }
    if chart.find("c:externalData").is_none() {
        return Err(unsupported("chart does not declare embedded data"));
    }
    let plot = chart.find("c:plotArea").ok_or_else(|| unsupported("chart has no plot area"))?;
    let kinds: Vec<_> = plot
        .els()
        .filter(|e| matches!(e.name.as_str(), "c:barChart" | "c:lineChart" | "c:areaChart" | "c:pieChart" | "c:doughnutChart" | "c:scatterChart"))
        .collect();
    if kinds.len() != 1 {
        return Err(unsupported("chart family is outside the supported bar, column, line, area, pie, doughnut, and scatter subset"));
    }
    let kind = kinds[0];
    let chart_kind = match kind.name.as_str() {
        "c:lineChart" => "line",
        "c:areaChart" => "area",
        "c:pieChart" => "pie",
        "c:doughnutChart" => "doughnut",
        "c:scatterChart" => "scatter",
        "c:barChart" => match kind.child("c:barDir").and_then(|e| e.attr("val")) {
            Some("col") => "column",
            Some("bar") => "bar",
            _ => return Err(unsupported("bar chart direction is missing or unsupported")),
        },
        _ => return Err(unsupported("chart family is unsupported")),
    };
    if contains_local(kind, "extLst", 0) {
        return Err(unsupported("supported chart types with extension markup are read-only"));
    }
    let series: Vec<_> = kind.children("c:ser").collect();
    if series.is_empty() || series.len() > MAX_SERIES {
        return Err(unsupported("chart has no series or exceeds the series limit"));
    }
    if matches!(chart_kind, "pie" | "doughnut") && series.len() != 1 {
        return Err(unsupported("editable pie and doughnut charts must contain exactly one series"));
    }
    let xlsx = Package::open(workbook).map_err(|_| unsupported("embedded workbook package is malformed or unsafe"))?;
    if xlsx.entries().any(|(part, _)| {
        let part = part.to_ascii_lowercase();
        part.starts_with("xl/externallinks/") || part.ends_with("/vbaproject.bin")
    }) {
        return Err(unsupported("workbook contains external-link or macro parts"));
    }
    let content_types = xlsx
        .xml("[Content_Types].xml")
        .map_err(|_| unsupported("workbook content types are malformed"))?
        .ok_or_else(|| unsupported("workbook content types are missing"))?;
    if content_types.local() != "Types" {
        return Err(unsupported("workbook content types root is invalid"));
    }
    let types = ContentTypes::read(&xlsx);
    if types.of("xl/workbook.xml") != Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml") {
        return Err(unsupported("workbook has an unsupported content type"));
    }
    let wb =
        xlsx.xml("xl/workbook.xml").map_err(|_| unsupported("workbook XML is malformed"))?.ok_or_else(|| unsupported("workbook.xml is missing"))?;
    let rels = xlsx.rels_checked("xl/workbook.xml").map_err(|_| unsupported("workbook relationships are malformed"))?;
    if rels.list.iter().any(|r| r.external) {
        return Err(unsupported("workbook contains external relationships"));
    }
    if rels.list.iter().any(|r| r.kind.to_ascii_lowercase().contains("externallink")) {
        return Err(unsupported("workbook contains external-link data"));
    }
    let mut sheets = BTreeMap::new();
    let sheet_nodes: Vec<_> = wb.find("x:sheets").ok_or_else(|| unsupported("workbook has no sheets list"))?.children("x:sheet").collect();
    if sheet_nodes.is_empty() {
        return Err(unsupported("workbook has no worksheets"));
    }
    for sheet in sheet_nodes {
        let (Some(name), Some(id)) = (sheet.attr("name"), sheet.attr("r:id")) else { return Err(unsupported("worksheet entry is incomplete")) };
        let rel = rels.by_id(id).ok_or_else(|| unsupported("worksheet relationship is missing"))?;
        if rel.external || !rel_is(&rel.kind, &format!("{XLSX_OFFICE}worksheet")) {
            return Err(unsupported("worksheet relationship is external or has an unexpected type"));
        }
        if !rel.target.starts_with("xl/") || rel.target.contains(['\\', ':']) {
            return Err(unsupported("worksheet relationship target is unsafe"));
        }
        if types.of(&rel.target) != Some("application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml") {
            return Err(unsupported("worksheet has an unsupported content type"));
        }
        if sheets.insert(name.to_string(), rel.target.clone()).is_some() {
            return Err(ambiguous("workbook contains duplicate worksheet names"));
        }
    }
    let shared = if let Some(shared_rel) = rels.by_type(&format!("{XLSX_OFFICE}sharedStrings")) {
        if shared_rel.external || !shared_rel.target.starts_with("xl/") || shared_rel.target.contains(['\\', ':']) {
            return Err(unsupported("shared strings relationship target is unsafe"));
        }
        let root = xlsx
            .xml(&shared_rel.target)
            .map_err(|_| unsupported("shared strings XML is malformed"))?
            .ok_or_else(|| unsupported("shared strings part is missing"))?;
        parse_shared_strings(&root)?
    } else {
        Vec::new()
    };
    let mut sheet_cache: BTreeMap<String, BTreeMap<String, CellValue>> = BTreeMap::new();
    let mut mapped = Vec::with_capacity(series.len());
    let mut mapped_cells = 0usize;
    for ser in series {
        let name_source = ser.child("c:tx").ok_or_else(|| unsupported("series name has no source"))?;
        let name_ref = ref_formula(name_source, "series name")?;
        if name_ref.cells.len() != 1 || name_source.child("c:strRef").is_none() {
            return Err(unsupported("series name must use one string-reference cell"));
        }
        let Some(path) = sheets.get(&name_ref.sheet) else { return Err(unsupported("chart range references a missing worksheet")) };
        if !sheet_cache.contains_key(path) {
            let root = xlsx
                .xml(path)
                .map_err(|_| unsupported("worksheet XML is malformed"))?
                .ok_or_else(|| unsupported("referenced worksheet part is missing"))?;
            let sheet_rels = xlsx.rels_checked(path).map_err(|_| unsupported("worksheet relationships are malformed"))?;
            if !sheet_rels.list.is_empty() {
                return Err(unsupported("worksheet has relationships outside the supported subset"));
            }
            sheet_cache.insert(path.clone(), parse_sheet(&root, &shared)?);
        }
        let cells = sheet_cache.get(path).ok_or_else(|| unsupported("worksheet mapping is unavailable"))?;
        let mut names = read_text_range(&name_ref, &name_ref.sheet, path, cells)?;
        let name = names.pop().ok_or_else(|| inconsistent("series name cell is missing"))?;
        verify_string_cache(ser.child("c:tx"), std::slice::from_ref(&name))?;
        mapped_cells = mapped_cells.checked_add(name_ref.cells.len()).ok_or_else(|| unsupported("mapped cell budget overflow"))?;
        let data = if chart_kind == "scatter" {
            if ser.child("c:cat").is_some() || ser.child("c:val").is_some() {
                return Err(unsupported("scatter series must use X/Y value references, not category/value references"));
            }
            let x_source = ser.child("c:xVal").ok_or_else(|| unsupported("scatter X values have no source"))?;
            let y_source = ser.child("c:yVal").ok_or_else(|| unsupported("scatter Y values have no source"))?;
            if x_source.child("c:numRef").is_none() || y_source.child("c:numRef").is_none() {
                return Err(unsupported("scatter X and Y values must use numeric worksheet references"));
            }
            let x_ref = ref_formula(x_source, "scatter X values")?;
            let y_ref = ref_formula(y_source, "scatter Y values")?;
            if x_ref.cells.is_empty() || x_ref.cells.len() != y_ref.cells.len() {
                return Err(ambiguous("scatter X and Y ranges must have the same nonzero number of cells"));
            }
            if name_ref.sheet != x_ref.sheet || x_ref.sheet != y_ref.sheet {
                return Err(unsupported("scatter series name and X/Y ranges must use the same worksheet"));
            }
            mapped_cells = mapped_cells
                .checked_add(x_ref.cells.len())
                .and_then(|count| count.checked_add(y_ref.cells.len()))
                .ok_or_else(|| unsupported("mapped cell budget overflow"))?;
            if mapped_cells > MAX_MAPPED_CELLS {
                return Err(unsupported("chart exceeds the mapped cell budget"));
            }
            let x_values = read_number_range(&x_ref, &x_ref.sheet, path, cells)?;
            let y_values = read_number_range(&y_ref, &y_ref.sheet, path, cells)?;
            verify_number_cache(Some(x_source), &x_values)?;
            verify_number_cache(Some(y_source), &y_values)?;
            SeriesDataMapping::Scatter { x_values, y_values }
        } else {
            let category_source = ser.child("c:cat").ok_or_else(|| unsupported("category labels have no source"))?;
            let value_source = ser.child("c:val").ok_or_else(|| unsupported("series values have no source"))?;
            if name_source.child("c:strRef").is_none() || category_source.child("c:strRef").is_none() || value_source.child("c:numRef").is_none() {
                return Err(unsupported("series name/category/value reference types are unsupported"));
            }
            let cat_ref = ref_formula(category_source, "category labels")?;
            let val_ref = ref_formula(value_source, "series values")?;
            if cat_ref.cells.len() != val_ref.cells.len() || cat_ref.cells.is_empty() {
                return Err(ambiguous("series ranges have incompatible dimensions"));
            }
            if name_ref.sheet != cat_ref.sheet || cat_ref.sheet != val_ref.sheet {
                return Err(unsupported("series ranges span different worksheets"));
            }
            mapped_cells = mapped_cells
                .checked_add(cat_ref.cells.len())
                .and_then(|count| count.checked_add(val_ref.cells.len()))
                .ok_or_else(|| unsupported("mapped cell budget overflow"))?;
            if mapped_cells > MAX_MAPPED_CELLS {
                return Err(unsupported("chart exceeds the mapped cell budget"));
            }
            let categories = read_text_range(&cat_ref, &cat_ref.sheet, path, cells)?;
            let values = read_number_range(&val_ref, &val_ref.sheet, path, cells)?;
            verify_string_cache(ser.child("c:cat"), &categories)?;
            verify_number_cache(ser.child("c:val"), &values)?;
            SeriesDataMapping::CategoryValues { categories, values }
        };
        let point_count = match &data {
            SeriesDataMapping::CategoryValues { values, .. } => values.len(),
            SeriesDataMapping::Scatter { y_values, .. } => y_values.len(),
        };
        mapped.push(SeriesMapping { name, data, formatting: inspect_series_format(ser, chart_kind, kind, point_count) });
    }
    if chart_kind == "scatter" {
        let mut sources = BTreeSet::new();
        for series in &mapped {
            let cells = std::iter::once((series.name.worksheet_part.as_str(), series.name.address.as_str())).chain(match &series.data {
                SeriesDataMapping::Scatter { x_values, y_values } => {
                    x_values.iter().chain(y_values).map(|cell| (cell.worksheet_part.as_str(), cell.address.as_str())).collect::<Vec<_>>()
                }
                SeriesDataMapping::CategoryValues { .. } => Vec::new(),
            });
            for cell in cells {
                if !sources.insert((cell.0.to_string(), cell.1.to_string())) {
                    return Err(ambiguous("scatter series have overlapping or duplicate source-cell references"));
                }
            }
        }
    }
    let legend_position = chart
        .child("c:chart")
        .filter(|chart| chart.children("c:legend").count() == 1)
        .and_then(|chart| chart.child("c:legend"))
        .filter(|legend| legend.children("c:legendPos").count() == 1)
        .and_then(|legend| legend.child("c:legendPos"))
        .and_then(|position| position.attr("val"))
        .filter(|position| matches!(*position, "l" | "r" | "t" | "b"))
        .map(str::to_string);
    let title_nodes = inspect_chart_title_nodes(chart_xml, chart_kind)?;
    let chart_title = title_nodes.chart.and_then(|index| editable_title_text(&title_nodes.nodes, index).map(|(_, value)| value));
    let horizontal_axis_title = title_nodes.horizontal_axis.and_then(|index| editable_title_text(&title_nodes.nodes, index).map(|(_, value)| value));
    let vertical_axis_title = title_nodes.vertical_axis.and_then(|index| editable_title_text(&title_nodes.nodes, index).map(|(_, value)| value));
    let (horizontal_axis, vertical_axis) = inspect_axis_settings(&chart, chart_kind);
    Ok(ChartMapping {
        chart_kind: chart_kind.into(),
        workbook_part: workbook_part.into(),
        legend_position,
        chart_title,
        horizontal_axis_title,
        vertical_axis_title,
        horizontal_axis,
        vertical_axis,
        series: mapped,
    })
}

fn inspect_series_format(series: &El, chart_kind: &str, chart_type: &El, point_count: usize) -> SeriesFormatSnapshot {
    let pie_varies = matches!(chart_kind, "pie" | "doughnut")
        && (chart_type.children("c:varyColors").count() != 1
            || chart_type.child("c:varyColors").and_then(|node| node.attr("val")).is_none_or(|value| !matches!(value, "0" | "false")));
    let properties_count = series.children("c:spPr").count();
    let color_target = if pie_varies || properties_count > 1 { None } else { series_color_target(series, chart_kind) };
    let color = color_target.and_then(simple_srgb_value);
    let color_editable = color.is_some();
    let line_count = series.child("c:spPr").map_or(0, |properties| properties.children("a:ln").count());
    let line = (line_count == 1).then(|| series.child("c:spPr").and_then(|properties| properties.child("a:ln"))).flatten();
    let line_width = line
        .and_then(|line| line.attr("w"))
        .and_then(|width| width.parse::<f64>().ok())
        .map(|width| width / 12_700.0)
        .filter(|width| width.is_finite() && (0.0..=20.0).contains(width));
    let line_width_editable = properties_count == 1 && matches!(chart_kind, "line" | "scatter") && line.is_some() && line_width.is_some();
    let marker = (chart_kind == "scatter" && series.children("c:marker").count() == 1).then(|| series.child("c:marker")).flatten();
    let symbol = marker.and_then(|marker| (marker.children("c:symbol").count() == 1).then(|| marker.child("c:symbol")).flatten());
    let marker_symbol = symbol.and_then(|symbol| symbol.attr("val")).filter(|symbol| supported_marker_symbol(symbol)).map(str::to_string);
    let marker_symbol_editable = marker_symbol.is_some() && symbol.is_some_and(simple_empty_val_element);
    let size = marker.and_then(|marker| (marker.children("c:size").count() == 1).then(|| marker.child("c:size")).flatten());
    let marker_size = size.and_then(|size| size.attr("val")).and_then(|size| size.parse::<u8>().ok()).filter(|size| (2..=72).contains(size));
    let marker_size_editable = marker_size.is_some() && size.is_some_and(simple_empty_val_element);
    let marker_fill = simple_marker_fill(marker);
    let marker_color = marker_fill.and_then(simple_srgb_value);
    let marker_color_editable =
        marker_color.is_some() && !color_target.is_some_and(|target| marker_fill.is_some_and(|fill| std::ptr::eq(target, fill)));
    let data_labels = inspect_data_labels(series, chart_kind);
    let error_bars = inspect_error_bars(series, chart_kind, point_count);
    let diagnostic = if pie_varies {
        Some("Pie and doughnut charts use per-slice colors; series color editing is read-only.".into())
    } else if !color_editable {
        Some("This series has a theme, gradient, mixed, or implicit color that cannot be changed safely.".into())
    } else if matches!(chart_kind, "line" | "scatter") && !line_width_editable {
        Some("Line width is read-only because there is no single explicit stroke width.".into())
    } else {
        None
    };
    SeriesFormatSnapshot {
        color,
        color_editable,
        line_width,
        line_width_editable,
        marker_symbol,
        marker_symbol_editable,
        marker_size,
        marker_size_editable,
        marker_color,
        marker_color_editable,
        data_labels,
        error_bars,
        diagnostic,
    }
}

fn inspect_error_bars(series: &El, chart_kind: &str, point_count: usize) -> Vec<ErrorBarSnapshot> {
    let mut snapshots: Vec<ErrorBarSnapshot> = series.children("c:errBars").map(|bar| {
        let direction = bar.child("c:errDir").and_then(|node| node.attr("val")).unwrap_or("unknown").to_string();
        let bar_type = bar.child("c:errBarType").and_then(|node| node.attr("val")).map(str::to_string);
        let value_type = bar.child("c:errValType").and_then(|node| node.attr("val")).map(str::to_string);
        let mut diagnostic = None;
        if !matches!(chart_kind, "bar" | "column" | "line" | "scatter") {
            diagnostic = Some("Error bars are currently rendered only for bar, line, and scatter charts.".into());
        } else if bar.children("c:errDir").count() != 1
            || bar.children("c:errBarType").count() != 1
            || bar.children("c:errValType").count() != 1
            || bar.children("c:noEndCap").count() > 1
        {
            diagnostic = Some("The error-bar definition has duplicate or missing required properties.".into());
        } else if !matches!(direction.as_str(), "x" | "y") || (direction == "x" && chart_kind != "scatter") {
            diagnostic = Some("This error-bar direction is missing or unsupported for the chart type.".into());
        } else if !matches!(bar_type.as_deref(), Some("both" | "plus" | "minus")) {
            diagnostic = Some("The error-bar direction type is missing or unsupported.".into());
        } else if point_count == 0 {
            diagnostic = Some("The series has no mapped data points to validate against.".into());
        } else {
            match value_type.as_deref() {
                Some("fixedVal") => {
                    let value = (bar.children("c:val").count() == 1)
                        .then(|| bar.child("c:val"))
                        .flatten()
                        .filter(|node| node.els().next().is_none())
                        .and_then(|node| node.attr("val"))
                        .and_then(|value| value.parse::<f64>().ok());
                    if !value.is_some_and(|value| value.is_finite() && value >= 0.0) {
                        diagnostic = Some("The fixed error-bar value is missing, negative, or non-finite.".into());
                    }
                }
                Some("cust") => {
                    let positive_needed = matches!(bar_type.as_deref(), Some("both" | "plus"));
                    let negative_needed = matches!(bar_type.as_deref(), Some("both" | "minus"));
                    let plus = bar.child("c:plus");
                    let minus = bar.child("c:minus");
                    let plus_ok = !positive_needed || valid_error_num_lit(plus, point_count);
                    let minus_ok = !negative_needed || valid_error_num_lit(minus, point_count);
                    let plus_count_ok = bar.children("c:plus").count() == usize::from(positive_needed);
                    let minus_count_ok = bar.children("c:minus").count() == usize::from(negative_needed);
                    if !plus_ok || !minus_ok || !plus_count_ok || !minus_count_ok {
                        diagnostic = Some(if bar.find("c:f").is_some() {
                            "Custom error-bar workbook ranges are retained but are not rendered until their cells can be verified against the embedded workbook.".into()
                        } else {
                            "Custom error-bar values are malformed or do not match the mapped series length.".into()
                        });
                    }
                }
                Some("percentage") => diagnostic = Some("Percentage error bars are preserved but not rendered yet.".into()),
                Some("stdDev") => diagnostic = Some("Standard-deviation bars require statistical calculation that is not modeled yet.".into()),
                Some("stdErr") => diagnostic = Some("Standard-error bars require statistical calculation that is not modeled yet.".into()),
                _ => diagnostic = Some("The error-bar value type is missing or unsupported.".into()),
            }
        }
        ErrorBarSnapshot { direction, bar_type, value_type, renderable: diagnostic.is_none(), diagnostic }
    }).collect();
    for direction in ["x", "y"] {
        let indices: Vec<usize> = snapshots.iter().enumerate().filter_map(|(index, bar)| (bar.direction == direction).then_some(index)).collect();
        if indices.len() > 1 {
            for index in indices {
                if let Some(bar) = snapshots.get_mut(index) {
                    bar.renderable = false;
                    bar.diagnostic = Some("Multiple error-bar definitions for the same axis are ambiguous.".into());
                }
            }
        }
    }
    snapshots
}

fn valid_error_num_lit(source: Option<&El>, expected: usize) -> bool {
    let Some(element) = source else { return false };
    if element.els().count() != 1 || element.children("c:numLit").count() != 1 {
        return false;
    }
    let Some(literal) = element.child("c:numLit") else { return false };
    if literal.children("c:ptCount").count() != 1 {
        return false;
    }
    let Some(count) = literal.child("c:ptCount").and_then(|node| node.attr("val")).and_then(|value| value.parse::<usize>().ok()) else {
        return false;
    };
    if count != expected || literal.children("c:pt").count() != expected {
        return false;
    }
    let mut seen = BTreeSet::new();
    literal.children("c:pt").all(|point| {
        let Some(index) = point.attr("idx").and_then(|value| value.parse::<usize>().ok()) else { return false };
        let Some(value) = point.child("c:v").and_then(|node| node.text().trim().parse::<f64>().ok()) else { return false };
        index < expected && seen.insert(index) && value.is_finite() && value >= 0.0
    }) && seen.len() == expected
}

fn inspect_data_labels(series: &El, chart_kind: &str) -> Option<DataLabelSnapshot> {
    let blocks: Vec<_> = series.children("c:dLbls").collect();
    let block = blocks.first().copied()?;
    let supported_positions: &[&str] = match chart_kind {
        "bar" | "column" => &["ctr", "inBase", "inEnd", "outEnd"],
        "line" | "area" | "scatter" => &["ctr", "t", "b", "l", "r"],
        "pie" | "doughnut" => &["ctr", "inEnd"],
        _ => &[],
    };
    let known = [
        "c:delete",
        "c:showLegendKey",
        "c:showVal",
        "c:showCatName",
        "c:showSerName",
        "c:showPercent",
        "c:showLeaderLines",
        "c:dLblPos",
        "c:numFmt",
        "c:separator",
        "c:showBubbleSize",
        "c:spPr",
        "c:txPr",
    ];
    let has_custom_points = block.children("c:dLbl").next().is_some();
    let has_unknown = block.els().any(|child| !known.contains(&child.name.as_str()));
    let deleted = block.child("c:delete").and_then(|node| node.attr("val")).is_some_and(|value| matches!(value, "1" | "true"));
    let simple = |name: &str| -> Option<bool> {
        let children: Vec<_> = block.children(name).collect();
        let node = children.first().copied()?;
        if children.len() != 1 || node.els().next().is_some() || !node.kids.is_empty() {
            return None;
        }
        match node.attr("val")? {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        }
    };
    let pos_nodes: Vec<_> = block.children("c:dLblPos").collect();
    let position = pos_nodes.first().and_then(|node| node.attr("val")).filter(|value| supported_positions.contains(value)).map(str::to_string);
    let editable = blocks.len() == 1 && !has_custom_points && !has_unknown && !deleted;
    let diagnostic = (!editable).then(|| {
        if has_custom_points {
            "Custom per-point data labels are preserved and read-only.".to_string()
        } else if deleted {
            "This series' labels are explicitly deleted and remain read-only.".to_string()
        } else {
            "This data-label structure is ambiguous or contains unsupported OOXML and remains read-only.".to_string()
        }
    });
    Some(DataLabelSnapshot {
        show_value: simple("c:showVal"),
        show_category: simple("c:showCatName"),
        show_series_name: simple("c:showSerName"),
        position: (pos_nodes.len() == 1).then_some(position.clone()).flatten(),
        editable: editable
            && (simple("c:showVal").is_some() || simple("c:showCatName").is_some() || simple("c:showSerName").is_some() || position.is_some()),
        diagnostic: diagnostic
            .or_else(|| (position.is_none() && pos_nodes.len() == 1).then(|| "This label position is not supported for the chart family.".into())),
    })
}

fn supported_marker_symbol(symbol: &str) -> bool {
    matches!(symbol, "circle" | "dot" | "square" | "triangle" | "diamond" | "star" | "plus" | "x" | "dash" | "none")
}

fn simple_empty_val_element(element: &El) -> bool {
    element.els().next().is_none() && element.kids.is_empty() && element.attr("val").is_some()
}

fn simple_marker_fill(marker: Option<&El>) -> Option<&El> {
    let marker = marker?;
    if marker.children("c:spPr").count() != 1 {
        return None;
    }
    let properties = marker.child("c:spPr")?;
    let fills: Vec<_> = properties
        .els()
        .filter(|element| matches!(element.name.as_str(), "a:solidFill" | "a:gradFill" | "a:pattFill" | "a:noFill" | "a:blipFill"))
        .collect();
    (fills.len() == 1 && fills.first().is_some_and(|fill| fill.name == "a:solidFill")).then(|| fills.first().copied()).flatten()
}

fn series_color_target<'a>(series: &'a El, chart_kind: &str) -> Option<&'a El> {
    let Some(properties) = series.child("c:spPr") else {
        if chart_kind == "scatter" {
            let markers: Vec<_> = series.children("c:marker").collect();
            if markers.len() != 1 {
                return None;
            }
            let marker_properties: Vec<_> = markers.first()?.children("c:spPr").collect();
            return (marker_properties.len() == 1).then(|| marker_properties.first()?.child("a:solidFill")).flatten();
        }
        return None;
    };
    if matches!(chart_kind, "line" | "scatter") {
        // These families draw their series color through a:ln. A competing fill can be
        // meaningful OOXML of its own, so keep it read-only instead of guessing precedence.
        if properties.els().any(|node| matches!(node.name.as_str(), "a:solidFill" | "a:gradFill" | "a:pattFill" | "a:blipFill")) {
            return None;
        }
        if let Some(line) = properties.child("a:ln") {
            if let Some(fill) = line.child("a:solidFill") {
                return Some(fill);
            }
            if line.els().any(|node| matches!(node.name.as_str(), "a:gradFill" | "a:pattFill" | "a:noFill")) {
                return None;
            }
        }
        if chart_kind == "scatter" {
            let markers: Vec<_> = series.children("c:marker").collect();
            if markers.len() != 1 {
                return None;
            }
            let properties: Vec<_> = markers.first()?.children("c:spPr").collect();
            return (properties.len() == 1).then(|| properties.first()?.child("a:solidFill")).flatten();
        }
        return None;
    }
    properties.child("a:solidFill")
}

fn simple_srgb_value(fill: &El) -> Option<String> {
    if fill.name != "a:solidFill" || fill.els().count() != 1 || fill.kids.len() != 1 {
        return None;
    }
    let color = fill.child("a:srgbClr")?;
    if color.els().next().is_some() || !color.kids.is_empty() {
        return None;
    }
    let value = color.attr("val")?;
    (value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| value.to_ascii_uppercase())
}

fn inspect_axis_settings(chart: &El, chart_kind: &str) -> (Option<ChartAxisSnapshot>, Option<ChartAxisSnapshot>) {
    if matches!(chart_kind, "pie" | "doughnut") {
        return (None, None);
    }
    let Some(plot) = chart.find("c:plotArea") else { return (None, None) };
    let kinds: Vec<_> =
        plot.els().filter(|element| matches!(element.name.as_str(), "c:barChart" | "c:lineChart" | "c:areaChart" | "c:scatterChart")).collect();
    let Some(kind) = (kinds.len() == 1).then(|| kinds[0]) else { return (None, None) };
    let ids: Vec<_> = kind.children("c:axId").filter_map(|node| node.attr("val")).collect();
    if ids.len() != 2 || ids.first() == ids.get(1) {
        return (None, None);
    }
    let axes: Vec<_> = plot
        .els()
        .filter(|axis| matches!(axis.name.as_str(), "c:catAx" | "c:dateAx" | "c:valAx"))
        .filter(|axis| axis.child("c:axId").and_then(|id| id.attr("val")).is_some_and(|id| ids.contains(&id)))
        .collect();
    if axes.len() != 2
        || ids.iter().any(|id| axes.iter().filter(|axis| axis.child("c:axId").and_then(|node| node.attr("val")) == Some(*id)).count() != 1)
    {
        return (None, None);
    }
    let mut horizontal = None;
    let mut vertical = None;
    for axis in axes {
        let Some(position) = axis.child("c:axPos").and_then(|node| node.attr("val")) else { return (None, None) };
        let slot = match position {
            "b" | "t" => &mut horizontal,
            "l" | "r" => &mut vertical,
            _ => return (None, None),
        };
        if slot.is_some() {
            return (None, None);
        }
        let value_axis = axis.local() == "valAx";
        let scatter = chart_kind == "scatter";
        let scaling = axis.child("c:scaling");
        let parse_bound = |name: &str| {
            scaling
                .and_then(|element| element.child(name))
                .and_then(|element| element.attr("val"))
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite())
        };
        let number_format =
            axis.child("c:numFmt").filter(|node| node.attr("sourceLinked") != Some("1")).and_then(|node| node.attr("formatCode")).map(str::to_string);
        let number_format_editable = number_format.as_deref().is_none_or(safe_axis_format);
        let unsafe_scaling = scaling.is_some_and(|scale| {
            scale.child("c:logBase").is_some()
                || scale.child("c:orientation").and_then(|orientation| orientation.attr("val")).is_some_and(|value| value != "minMax")
                || scale.els().any(|child| !matches!(child.name.as_str(), "c:logBase" | "c:orientation" | "c:max" | "c:min" | "c:extLst"))
        });
        let invalid_bounds = scaling.is_some_and(|scale| {
            ["c:min", "c:max"].into_iter().any(|name| {
                scale.children(name).count() > 1
                    || scale
                        .child(name)
                        .is_some_and(|bound| bound.attr("val").and_then(|value| value.parse::<f64>().ok()).is_none_or(|value| !value.is_finite()))
            })
        });
        // Editing known fields in an axis with an unfamiliar direct child risks changing
        // semantics that this version does not understand. Keep the whole axis read-only.
        let unsupported_axis_children = axis.els().any(|child| {
            !matches!(
                child.name.as_str(),
                "c:axId"
                    | "c:scaling"
                    | "c:delete"
                    | "c:axPos"
                    | "c:majorGridlines"
                    | "c:minorGridlines"
                    | "c:title"
                    | "c:numFmt"
                    | "c:majorTickMark"
                    | "c:minorTickMark"
                    | "c:tickLblPos"
                    | "c:spPr"
                    | "c:txPr"
                    | "c:crossAx"
                    | "c:crosses"
                    | "c:crossesAt"
                    | "c:crossBetween"
                    | "c:majorUnit"
                    | "c:minorUnit"
                    | "c:dispUnits"
                    | "c:extLst"
                    | "c:auto"
                    | "c:lblOffset"
                    | "c:tickLblSkip"
                    | "c:tickMarkSkip"
                    | "c:noMultiLvlLabels"
                    | "c:baseTimeUnit"
            )
        });
        let role = if !value_axis {
            "category"
        } else if scatter && position == "b" || scatter && position == "t" {
            "scatterX"
        } else if scatter {
            "scatterY"
        } else {
            "value"
        };
        let editable = value_axis
            && !unsafe_scaling
            && !invalid_bounds
            && !unsupported_axis_children
            && axis.children("c:scaling").count() <= 1
            && axis.children("c:numFmt").count() <= 1;
        *slot = Some(ChartAxisSnapshot {
            role: role.to_string(),
            minimum: parse_bound("c:min"),
            maximum: parse_bound("c:max"),
            number_format,
            editable,
            number_format_editable: editable && number_format_editable,
            diagnostic: if !value_axis {
                Some("Category axes are read-only; scaling controls apply only to numeric value axes.".into())
            } else if unsafe_scaling {
                Some("Logarithmic or reversed axes are read-only.".into())
            } else if invalid_bounds {
                Some("The existing axis bounds are malformed or duplicated.".into())
            } else if unsupported_axis_children {
                Some("This axis contains unsupported OOXML and remains read-only.".into())
            } else if !editable {
                Some("This axis has ambiguous or unsupported OOXML properties.".into())
            } else if !number_format_editable {
                Some("The existing custom number format is preserved and cannot be changed safely.".into())
            } else {
                None
            },
        });
    }
    (horizontal, vertical)
}

fn safe_axis_format(format: &str) -> bool {
    matches!(format, "General" | "0" | "#,##0" | "0.0" | "0.00" | "0%" | "0.0%" | "0.00%" | "0.00E+00")
}

fn contains_local(root: &El, local: &str, depth: usize) -> bool {
    depth <= 256 && (root.local() == local || root.els().any(|child| contains_local(child, local, depth + 1)))
}

fn ref_formula(container: &El, label: &str) -> MResult<Range> {
    if container.find("c:strLit").is_some() || container.find("c:numLit").is_some() {
        return Err(unsupported(&format!("{label} is a literal, not worksheet-backed")));
    }
    let formula = container.find("c:f").ok_or_else(|| unsupported(&format!("{label} has no range formula")))?.text();
    parse_range(&formula)
}

fn parse_range(formula: &str) -> MResult<Range> {
    if formula.is_empty() || formula.len() > 4096 || formula.contains(['[', ']', ',', ';', '(', ')', '\n', '\r']) || formula.matches('!').count() != 1
    {
        return Err(unsupported("range expression is external, noncontiguous or malformed"));
    }
    let (sheet, cell_range) = formula.split_once('!').ok_or_else(|| unsupported("range has no worksheet qualifier"))?;
    let sheet = if let Some(quoted) = sheet.strip_prefix('\'') {
        let Some(unquoted) = quoted.strip_suffix('\'') else { return Err(unsupported("worksheet name quoting is malformed")) };
        if unquoted.contains('\'') {
            return Err(unsupported("escaped worksheet names are unsupported"));
        }
        unquoted
    } else {
        if sheet.contains(' ') || sheet.contains('\'') {
            return Err(unsupported("worksheet names with spaces must be quoted"));
        }
        sheet
    };
    if sheet.is_empty() || sheet.contains(['/', '\\', ':']) || cell_range.contains(' ') {
        return Err(unsupported("worksheet name is malformed"));
    }
    let (start, end) = cell_range.split_once(':').unwrap_or((cell_range, cell_range));
    let a = parse_cell(start)?;
    let b = parse_cell(end)?;
    if a.col != b.col || b.row < a.row {
        return Err(unsupported("only a contiguous single-column cell range is supported"));
    }
    let count = usize::try_from(b.row - a.row + 1).map_err(|_| unsupported("range length overflow"))?;
    if count == 0 || count > MAX_POINTS {
        return Err(unsupported("range exceeds the chart mapping point limit"));
    }
    let cells = (a.row..=b.row).map(|row| CellAddress { col: a.col, row }).collect();
    Ok(Range { sheet: sheet.to_string(), cells })
}

fn parse_cell(s: &str) -> MResult<CellAddress> {
    if s.len() > 16 {
        return Err(unsupported("cell reference exceeds the length limit"));
    }
    let s = s.replace('$', "");
    let split = s.find(|c: char| c.is_ascii_digit()).ok_or_else(|| unsupported("cell reference has no row"))?;
    let (letters, digits) = s.split_at(split);
    if letters.is_empty() || !letters.bytes().all(|b| b.is_ascii_alphabetic()) || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(unsupported("cell reference is malformed"));
    }
    let mut col = 0u32;
    for c in letters.bytes() {
        col = col
            .checked_mul(26)
            .and_then(|v| v.checked_add(u32::from(c.to_ascii_uppercase() - b'A' + 1)))
            .ok_or_else(|| unsupported("cell column overflow"))?;
    }
    let row = digits.parse::<u32>().map_err(|_| unsupported("cell row is malformed"))?;
    if row == 0 || col == 0 || col > 16_384 || row > 1_048_576 {
        return Err(unsupported("cell is outside the worksheet bounds"));
    }
    Ok(CellAddress { col, row })
}

fn cell_name(address: &CellAddress) -> String {
    let mut col = address.col;
    let mut chars = Vec::new();
    while col > 0 {
        let rem = ((col - 1) % 26) as u8;
        chars.push(char::from(b'A' + rem));
        col = (col - 1) / 26;
    }
    chars.reverse();
    format!("{}{row}", chars.into_iter().collect::<String>(), row = address.row)
}

fn parse_shared_strings(root: &El) -> MResult<Vec<String>> {
    if root.local() != "sst" {
        return Err(unsupported("shared strings root is invalid"));
    }
    Ok(root.children("x:si").map(El::deep_text).collect())
}

fn parse_sheet(root: &El, shared: &[String]) -> MResult<BTreeMap<String, CellValue>> {
    if root.local() != "worksheet" {
        return Err(unsupported("worksheet root is invalid"));
    }
    let mut cells = BTreeMap::new();
    let Some(data) = root.child("x:sheetData") else { return Err(unsupported("worksheet has no cell data")) };
    for row in data.children("x:row") {
        let row_number = row.attr("r").and_then(|value| value.parse::<u32>().ok()).ok_or_else(|| unsupported("worksheet row index is malformed"))?;
        for cell in row.children("x:c") {
            let raw_address = cell.attr("r").ok_or_else(|| unsupported("worksheet cell has no address"))?;
            let parsed = parse_cell(raw_address)?;
            if parsed.row != row_number {
                return Err(unsupported("worksheet cell address does not match its row"));
            }
            let address = cell_name(&parsed);
            if cells.contains_key(&address) {
                return Err(ambiguous("worksheet contains duplicate cell addresses"));
            }
            if cell.child("x:f").is_some() {
                return Err(unsupported("formula-driven worksheet cells are read-only"));
            }
            let value = match cell.attr("t").unwrap_or("n") {
                "s" => {
                    let index = cell
                        .child("x:v")
                        .and_then(|e| e.text().parse::<usize>().ok())
                        .ok_or_else(|| unsupported("shared-string cell has an invalid index"))?;
                    CellValue::Text(
                        shared.get(index).ok_or_else(|| unsupported("shared-string cell index is missing"))?.clone(),
                        CellSource::SharedString(index),
                    )
                }
                "inlineStr" => CellValue::Text(
                    cell.child("x:is").ok_or_else(|| unsupported("inline string cell has no value"))?.deep_text(),
                    CellSource::InlineString,
                ),
                "str" => CellValue::Text(cell.child("x:v").map(El::text).unwrap_or_default(), CellSource::StringValue),
                "n" => {
                    let n = cell
                        .child("x:v")
                        .and_then(|v| v.text().parse::<f64>().ok())
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| unsupported("numeric cell is missing or invalid"))?;
                    CellValue::Number(n)
                }
                "b" | "e" | "d" => CellValue::Unsupported("boolean, error or date cell types are unsupported".into()),
                _ => CellValue::Unsupported("worksheet cell uses an unsupported value type".into()),
            };
            cells.insert(address, value);
        }
    }
    Ok(cells)
}

/// Inspect every rendered chart in the document using its retained package graph. This is the
/// read-only bridge between document preservation and the typed mapper; it never changes bytes.
pub(crate) fn inspect_document(doc: &wordcraft_doc::Document) -> Vec<Classification> {
    use wordcraft_doc::{Block, InlineObject};
    let manifest = doc.passthrough.get(EMBEDDED_PARTS).map(|bytes| EmbeddedManifest::parse(bytes)).unwrap_or_default();
    let mut blocks: Vec<&Block> = doc.body.iter().map(std::sync::Arc::as_ref).collect();
    for part in doc.parts.values() {
        blocks.extend(part.blocks.iter().map(std::sync::Arc::as_ref));
    }
    let mut found = Vec::new();
    while let Some(block) = blocks.pop() {
        match block {
            Block::Para(para) => {
                for object in &para.objects {
                    let InlineObject::Graphic { graphic, .. } = object else { continue };
                    if graphic.kind != wordcraft_doc::graphic::GraphicKind::Chart {
                        continue;
                    }
                    let chart_target =
                        graphic.source.as_ref().and_then(|s| s.refs.iter().find(|r| !r.external && rel_is(&r.kind, &format!("{XLSX_OFFICE}chart"))));
                    let Some(chart_ref) = chart_target else {
                        found.push(Classification::Unsupported("chart part relationship is missing".into()));
                        continue;
                    };
                    let (Some(chart_bytes), Some(chart_info)) = (doc.passthrough.get(&chart_ref.target), manifest.parts.get(&chart_ref.target))
                    else {
                        found.push(Classification::Unsupported("preserved chart part is missing".into()));
                        continue;
                    };
                    let workbook_rel = chart_info.rels.iter().find(|r| rel_is(&r.kind, &format!("{XLSX_OFFICE}package")));
                    let Some(workbook_rel) = workbook_rel else {
                        found.push(Classification::Unsupported("embedded workbook relationship is missing".into()));
                        continue;
                    };
                    let Some(workbook) = doc.passthrough.get(&workbook_rel.target) else {
                        found.push(Classification::Unsupported("embedded workbook part is missing".into()));
                        continue;
                    };
                    found.push(map_chart(chart_bytes, &chart_info.rels, &workbook_rel.target, workbook));
                }
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        blocks.extend(cell.blocks.iter().map(std::sync::Arc::as_ref));
                    }
                }
            }
        }
    }
    found
}

/// Whether the current document model contains a chart graphic linked to `chart_part`.
/// This prevents callers from using a package-only chart name to edit an unrelated/orphan part.
pub fn document_has_chart(doc: &wordcraft_doc::Document, chart_part: &str) -> bool {
    use wordcraft_doc::{Block, InlineObject};
    let mut blocks: Vec<&Block> = doc.body.iter().map(std::sync::Arc::as_ref).collect();
    for part in doc.parts.values() {
        blocks.extend(part.blocks.iter().map(std::sync::Arc::as_ref));
    }
    while let Some(block) = blocks.pop() {
        match block {
            Block::Para(para) => {
                if para.objects.iter().any(|object| {
                    let InlineObject::Graphic { graphic, .. } = object else { return false };
                    graphic.kind == wordcraft_doc::graphic::GraphicKind::Chart
                        && graphic.source.as_ref().is_some_and(|source| {
                            source.refs.iter().any(|reference| {
                                !reference.external && reference.target == chart_part && rel_is(&reference.kind, &format!("{XLSX_OFFICE}chart"))
                            })
                        })
                }) {
                    return true;
                }
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        blocks.extend(cell.blocks.iter().map(std::sync::Arc::as_ref));
                    }
                }
            }
        }
    }
    false
}

fn text_at(range: &Range, sheet: &str, sheet_path: &str, cells: &BTreeMap<String, CellValue>, index: usize) -> MResult<MappedCell<String>> {
    let address = range.cells.get(index).ok_or_else(|| inconsistent("range cell is missing"))?;
    let key = cell_name(address);
    let value = match cells.get(&key) {
        Some(CellValue::Text(value, _)) => value.clone(),
        Some(CellValue::Number(_)) => return Err(inconsistent("text chart reference points to a numeric cell")),
        Some(CellValue::Unsupported(reason)) => return Err(unsupported(reason)),
        None => return Err(inconsistent("chart range references a missing worksheet cell")),
    };
    if value.len() > MAX_LABEL_BYTES {
        return Err(unsupported("worksheet label exceeds the text limit"));
    }
    let source = match cells.get(&key) {
        Some(CellValue::Text(_, source)) => source.clone(),
        _ => return Err(inconsistent("text chart cell source is unavailable")),
    };
    Ok(MappedCell { worksheet: sheet.to_string(), worksheet_part: sheet_path.to_string(), address: key, value, source })
}

fn read_text_range(range: &Range, sheet: &str, sheet_path: &str, cells: &BTreeMap<String, CellValue>) -> MResult<Vec<MappedCell<String>>> {
    (0..range.cells.len()).map(|i| text_at(range, sheet, sheet_path, cells, i)).collect()
}

fn read_number_range(range: &Range, sheet: &str, sheet_path: &str, cells: &BTreeMap<String, CellValue>) -> MResult<Vec<MappedCell<f64>>> {
    range
        .cells
        .iter()
        .map(|addr| {
            let key = cell_name(addr);
            let value = match cells.get(&key) {
                Some(CellValue::Number(value)) => *value,
                Some(CellValue::Text(_, _)) => return Err(inconsistent("numeric chart reference points to a text cell")),
                Some(CellValue::Unsupported(reason)) => return Err(unsupported(reason)),
                None => return Err(inconsistent("chart range references a missing worksheet cell")),
            };
            Ok(MappedCell { worksheet: sheet.to_string(), worksheet_part: sheet_path.to_string(), address: key, value, source: CellSource::Number })
        })
        .collect()
}

fn cache_points(container: Option<&El>, cache_name: &str) -> MResult<Vec<(usize, String)>> {
    let cache = container.and_then(|e| e.find(cache_name)).ok_or_else(|| inconsistent("chart cache is missing"))?;
    let count = cache
        .child("c:ptCount")
        .and_then(|e| e.attr("val"))
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| inconsistent("chart cache point count is invalid"))?;
    if count > MAX_POINTS {
        return Err(unsupported("chart cache exceeds the point limit"));
    }
    let mut points = Vec::new();
    for point in cache.children("c:pt") {
        let index = point.attr("idx").and_then(|s| s.parse::<usize>().ok()).ok_or_else(|| inconsistent("chart cache has an invalid point index"))?;
        let value = point.child("c:v").map(El::text).ok_or_else(|| inconsistent("chart cache point has no value"))?;
        points.push((index, value));
    }
    points.sort_by_key(|(index, _)| *index);
    if points.len() != count || points.iter().enumerate().any(|(i, (index, _))| *index != i) {
        return Err(inconsistent("chart cache has missing, repeated or nonsequential points"));
    }
    Ok(points)
}

fn verify_string_cache(container: Option<&El>, values: &[MappedCell<String>]) -> MResult<()> {
    let pts = cache_points(container, "c:strCache")?;
    if pts.len() != values.len() || pts.iter().zip(values).any(|((_, cache), cell)| cache != &cell.value) {
        return Err(inconsistent("chart string cache does not match worksheet values or range length"));
    }
    Ok(())
}

fn verify_number_cache(container: Option<&El>, values: &[MappedCell<f64>]) -> MResult<()> {
    let pts = cache_points(container, "c:numCache")?;
    if pts.len() != values.len() {
        return Err(inconsistent("chart numeric cache length does not match worksheet range"));
    }
    for ((_, cache), cell) in pts.iter().zip(values) {
        let cached = cache.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| inconsistent("chart cache has a nonnumeric value"))?;
        if cached != cell.value {
            return Err(inconsistent("chart numeric cache does not match worksheet values"));
        }
    }
    Ok(())
}

const CHART_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const SHEET_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const DOC_REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CHART_CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
const XLSX_CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const MAX_MUTATION_XML: usize = 16 * 1024 * 1024;

/// An immutable authorization for one exact DOCX package and one validated chart mapping.
/// The digest prevents a caller from applying changes after either package part or any unrelated
/// package content has changed since the mapping was produced.
#[derive(Clone, Debug)]
#[allow(dead_code)] // Internal transaction API is intentionally staged before editor integration.
pub struct ChartEditPlan {
    chart_part: String,
    workbook_part: String,
    package_digest: [u8; 32],
    mapping: ChartMapping,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChartDataChanges {
    pub series: Vec<SeriesChanges>,
    #[serde(default)]
    pub legend_position: Option<String>,
    #[serde(default)]
    pub chart_title: Option<String>,
    #[serde(default)]
    pub horizontal_axis_title: Option<String>,
    #[serde(default)]
    pub vertical_axis_title: Option<String>,
    #[serde(default)]
    pub horizontal_axis: Option<AxisChanges>,
    #[serde(default)]
    pub vertical_axis: Option<AxisChanges>,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeriesChanges {
    pub name: Option<String>,
    pub categories: Option<Vec<String>>,
    pub color: Option<String>,
    pub line_width: Option<f64>,
    #[serde(default)]
    pub marker_symbol: Option<String>,
    #[serde(default)]
    pub marker_size: Option<u8>,
    #[serde(default)]
    pub marker_color: Option<String>,
    #[serde(default)]
    pub data_labels: Option<DataLabelChanges>,
    #[serde(default)]
    pub x_values: Option<Vec<f64>>,
    pub values: Option<Vec<f64>>,
}

/// Values shown in the chart data editor after the chart and its embedded workbook have been
/// validated against one another. This contains copies only; callers cannot use it to mutate a
/// package.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartDataSnapshot {
    pub chart_kind: String,
    #[serde(default)]
    pub legend_position: Option<String>,
    #[serde(default)]
    pub chart_title: Option<String>,
    #[serde(default)]
    pub horizontal_axis_title: Option<String>,
    #[serde(default)]
    pub vertical_axis_title: Option<String>,
    #[serde(default)]
    pub horizontal_axis: Option<ChartAxisSnapshot>,
    #[serde(default)]
    pub vertical_axis: Option<ChartAxisSnapshot>,
    pub series: Vec<ChartSeriesSnapshot>,
}

/// One validated series for the read-only chart editor view.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartSeriesSnapshot {
    pub name: String,
    pub categories: Vec<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub color_editable: bool,
    #[serde(default)]
    pub line_width: Option<f64>,
    #[serde(default)]
    pub line_width_editable: bool,
    #[serde(default)]
    pub marker_symbol: Option<String>,
    #[serde(default)]
    pub marker_symbol_editable: bool,
    #[serde(default)]
    pub marker_size: Option<u8>,
    #[serde(default)]
    pub marker_size_editable: bool,
    #[serde(default)]
    pub marker_color: Option<String>,
    #[serde(default)]
    pub marker_color_editable: bool,
    #[serde(default)]
    pub data_labels: Option<DataLabelSnapshot>,
    #[serde(default)]
    pub error_bars: Vec<ErrorBarSnapshot>,
    #[serde(default)]
    pub formatting_diagnostic: Option<String>,
    #[serde(default)]
    pub x_values: Vec<f64>,
    pub values: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TextTarget {
    worksheet_part: String,
    address: String,
}

#[derive(Clone, Debug, PartialEq)]
enum CellEdit {
    Text(String),
    Number(f64),
}

#[derive(Clone, Debug)]
struct Splice {
    range: std::ops::Range<usize>,
    replacement: Vec<u8>,
}

/// Create a transaction plan only for a chart that the read-only mapper validates.
pub fn prepare_chart_edit(docx: &[u8], chart_part: &str) -> Result<ChartEditPlan, DocxError> {
    let package = Package::open(docx)?;
    crate::preservation::validate(&package)?;
    if !safe_package_part(chart_part) {
        return Err(transaction_error("chart part path is unsafe"));
    }
    let content_types = ContentTypes::read(&package);
    if content_types.of(chart_part) != Some(CHART_CONTENT_TYPE) {
        return Err(transaction_error("chart part has a missing or unsupported content type"));
    }
    let chart = package.get(chart_part).ok_or_else(|| transaction_error("chart part is missing"))?;
    if !has_incoming_chart_relationship(&package, chart_part)? {
        return Err(transaction_error("chart part has no package relationship to it"));
    }
    let chart_rels = package.rels_checked(chart_part)?;
    let workbook_rels: Vec<_> = chart_rels.list.iter().filter(|r| rel_is(&r.kind, &format!("{XLSX_OFFICE}package"))).collect();
    if workbook_rels.len() != 1 {
        return Err(transaction_error("chart must have exactly one embedded workbook relationship"));
    }
    let workbook_rel = workbook_rels.first().ok_or_else(|| transaction_error("embedded workbook relationship is unavailable"))?;
    if workbook_rel.external || !safe_package_part(&workbook_rel.target) {
        return Err(transaction_error("external or unsafe workbook relationships cannot be edited"));
    }
    let workbook = package.get(&workbook_rel.target).ok_or_else(|| transaction_error("embedded workbook part is missing"))?;
    let mapping = match map_chart(chart, &chart_rels.list, &workbook_rel.target, workbook) {
        Classification::Mappable(mapping) => *mapping,
        Classification::Unsupported(reason) => return Err(transaction_error(format!("chart is read-only: {reason}"))),
        Classification::Ambiguous(reason) => return Err(transaction_error(format!("chart mapping is ambiguous: {reason}"))),
        Classification::Inconsistent(reason) => return Err(transaction_error(format!("chart data is inconsistent: {reason}"))),
    };
    let workbook_types = ContentTypes::read(&Package::open(workbook)?);
    if workbook_types.of("xl/workbook.xml") != Some(XLSX_CONTENT_TYPE) {
        return Err(transaction_error("embedded workbook content type is unsupported"));
    }
    Ok(ChartEditPlan {
        chart_part: chart_part.to_string(),
        workbook_part: workbook_rel.target.clone(),
        package_digest: Sha256::digest(docx).into(),
        mapping,
    })
}

/// Read the currently validated chart data for a chart editor. Unsupported charts fail with a
/// reason suitable for a read-only explanation in the UI.
pub fn inspect_chart_data(docx: &[u8], chart_part: &str) -> Result<ChartDataSnapshot, DocxError> {
    let plan = prepare_chart_edit(docx, chart_part)?;
    Ok(ChartDataSnapshot {
        chart_kind: plan.mapping.chart_kind,
        legend_position: plan.mapping.legend_position,
        chart_title: plan.mapping.chart_title,
        horizontal_axis_title: plan.mapping.horizontal_axis_title,
        vertical_axis_title: plan.mapping.vertical_axis_title,
        horizontal_axis: plan.mapping.horizontal_axis,
        vertical_axis: plan.mapping.vertical_axis,
        series: plan
            .mapping
            .series
            .into_iter()
            .map(|series| {
                let (categories, x_values, values) = match series.data {
                    SeriesDataMapping::CategoryValues { categories, values } => {
                        (categories.into_iter().map(|cell| cell.value).collect(), Vec::new(), values.into_iter().map(|cell| cell.value).collect())
                    }
                    SeriesDataMapping::Scatter { x_values, y_values } => {
                        (Vec::new(), x_values.into_iter().map(|cell| cell.value).collect(), y_values.into_iter().map(|cell| cell.value).collect())
                    }
                };
                ChartSeriesSnapshot {
                    name: series.name.value,
                    categories,
                    color: series.formatting.color,
                    color_editable: series.formatting.color_editable,
                    line_width: series.formatting.line_width,
                    line_width_editable: series.formatting.line_width_editable,
                    marker_symbol: series.formatting.marker_symbol,
                    marker_symbol_editable: series.formatting.marker_symbol_editable,
                    marker_size: series.formatting.marker_size,
                    marker_size_editable: series.formatting.marker_size_editable,
                    marker_color: series.formatting.marker_color,
                    marker_color_editable: series.formatting.marker_color_editable,
                    data_labels: series.formatting.data_labels,
                    error_bars: series.formatting.error_bars,
                    formatting_diagnostic: series.formatting.diagnostic,
                    x_values,
                    values,
                }
            })
            .collect(),
    })
}

/// Apply a constrained chart edit as one all-or-nothing package transaction.
///
/// No caller-owned bytes are mutated. Both chart caches and exactly mapped workbook cells are
/// staged, revalidated, and returned as a new DOCX only after package and mapping validation pass.
pub fn apply_chart_edit(docx: &[u8], plan: &ChartEditPlan, changes: &ChartDataChanges) -> Result<Vec<u8>, DocxError> {
    if Sha256::digest(docx).as_slice() != plan.package_digest {
        return Err(transaction_error("stale chart mapping: original package bytes have changed"));
    }
    validate_changes(&plan.mapping, changes)?;
    let package = Package::open(docx)?;
    crate::preservation::validate(&package)?;
    let chart = package.get(&plan.chart_part).ok_or_else(|| transaction_error("planned chart part is missing"))?;
    let rels = package.rels_checked(&plan.chart_part)?;
    let workbook_rel = rels
        .list
        .iter()
        .find(|r| rel_is(&r.kind, &format!("{XLSX_OFFICE}package")) && !r.external)
        .ok_or_else(|| transaction_error("planned embedded workbook relationship is missing"))?;
    if workbook_rel.target != plan.workbook_part {
        return Err(transaction_error("stale chart mapping: workbook relationship changed"));
    }
    let workbook = package.get(&plan.workbook_part).ok_or_else(|| transaction_error("planned workbook part is missing"))?;
    let fresh = match map_chart(chart, &rels.list, &plan.workbook_part, workbook) {
        Classification::Mappable(mapping) => *mapping,
        Classification::Unsupported(reason) => return Err(transaction_error(format!("chart became read-only: {reason}"))),
        Classification::Ambiguous(reason) => return Err(transaction_error(format!("chart mapping became ambiguous: {reason}"))),
        Classification::Inconsistent(reason) => return Err(transaction_error(format!("chart data became inconsistent: {reason}"))),
    };
    if fresh != plan.mapping {
        return Err(transaction_error("stale chart mapping: mapped cells or source values changed"));
    }

    let mut expected = fresh.clone();
    let chart_bytes = update_chart_caches(chart, &fresh, changes)?;
    let chart_bytes = apply_series_format_changes(&chart_bytes, &fresh, changes)?;
    let chart_bytes = apply_data_label_changes(&chart_bytes, &fresh, changes)?;
    let chart_bytes = apply_axis_changes(&chart_bytes, &fresh, changes)?;
    let cell_edits = build_cell_edits(&fresh, changes, &mut expected)?;
    for (index, change) in changes.series.iter().enumerate() {
        let expected_series =
            expected.series.get_mut(index).ok_or_else(|| transaction_error("chart series mapping changed during formatting edit"))?;
        if let Some(color) = &change.color {
            let shared_marker_color = expected_series.formatting.marker_color == expected_series.formatting.color;
            expected_series.formatting.color = Some(color.to_ascii_uppercase());
            if shared_marker_color {
                expected_series.formatting.marker_color = Some(color.to_ascii_uppercase());
            }
        }
        if let Some(width) = change.line_width {
            expected_series.formatting.line_width = Some((width * 12_700.0).round() / 12_700.0);
        }
        if let Some(symbol) = &change.marker_symbol {
            expected_series.formatting.marker_symbol = Some(symbol.clone());
        }
        if let Some(size) = change.marker_size {
            expected_series.formatting.marker_size = Some(size);
        }
        if let Some(color) = &change.marker_color {
            expected_series.formatting.marker_color = Some(color.to_ascii_uppercase());
        }
        if let (Some(label_changes), Some(labels)) = (&change.data_labels, expected_series.formatting.data_labels.as_mut()) {
            if let Some(value) = label_changes.show_value {
                labels.show_value = Some(value);
            }
            if let Some(value) = label_changes.show_category {
                labels.show_category = Some(value);
            }
            if let Some(value) = label_changes.show_series_name {
                labels.show_series_name = Some(value);
            }
            if let Some(value) = &label_changes.position {
                labels.position = Some(value.clone());
            }
        }
    }
    if let Some(position) = &changes.legend_position {
        expected.legend_position = Some(position.clone());
    }
    if let Some(title) = &changes.chart_title {
        expected.chart_title = Some(title.clone());
    }
    if let Some(title) = &changes.horizontal_axis_title {
        expected.horizontal_axis_title = Some(title.clone());
    }
    if let Some(title) = &changes.vertical_axis_title {
        expected.vertical_axis_title = Some(title.clone());
    }
    if let Some(axis) = &changes.horizontal_axis {
        expected.horizontal_axis = apply_axis_snapshot(expected.horizontal_axis.as_ref(), axis)?;
    }
    if let Some(axis) = &changes.vertical_axis {
        expected.vertical_axis = apply_axis_snapshot(expected.vertical_axis.as_ref(), axis)?;
    }
    let workbook_bytes = if cell_edits.is_empty() { workbook.to_vec() } else { apply_workbook_edits(workbook, &cell_edits)? };
    let mut replacements = BTreeMap::from([(plan.chart_part.clone(), chart_bytes), (plan.workbook_part.clone(), workbook_bytes)]);

    let entries: Vec<(String, Vec<u8>)> =
        package.entries().map(|(name, bytes)| (name.to_string(), replacements.remove(name).unwrap_or_else(|| bytes.to_vec()))).collect();
    if !replacements.is_empty() {
        return Err(transaction_error("transaction attempted to add or replace an unplanned package part"));
    }
    let output = zip_entries(&entries)?;
    let reopened = Package::open(&output)?;
    crate::preservation::validate(&reopened)?;
    let result_chart = reopened.get(&plan.chart_part).ok_or_else(|| transaction_error("updated chart part is missing"))?;
    let result_rels = reopened.rels_checked(&plan.chart_part)?;
    let result_workbook = reopened.get(&plan.workbook_part).ok_or_else(|| transaction_error("updated workbook part is missing"))?;
    match map_chart(result_chart, &result_rels.list, &plan.workbook_part, result_workbook) {
        Classification::Mappable(mapping) if *mapping == expected => Ok(output),
        Classification::Mappable(_) => Err(transaction_error("post-update chart/workbook values do not match the requested edit")),
        Classification::Unsupported(reason) | Classification::Ambiguous(reason) | Classification::Inconsistent(reason) => {
            Err(transaction_error(format!("post-update validation failed: {reason}")))
        }
    }
}

fn transaction_error(reason: impl Into<String>) -> DocxError {
    DocxError::Preservation(format!("chart transaction: {}", reason.into()))
}

fn safe_package_part(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains(['\\', ':', '#', '?'])
        && path.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn has_incoming_chart_relationship(package: &Package, chart_part: &str) -> Result<bool, DocxError> {
    for (path, _) in package.entries().filter(|(path, _)| path.ends_with(".rels")) {
        let owner = if path == "_rels/.rels" {
            String::new()
        } else {
            let (directory, filename) = path.rsplit_once('/').ok_or_else(|| transaction_error("invalid relationship path"))?;
            let directory =
                directory.strip_suffix("_rels").ok_or_else(|| transaction_error("relationship outside an _rels directory"))?.trim_end_matches('/');
            let filename = filename.strip_suffix(".rels").ok_or_else(|| transaction_error("invalid relationship filename"))?;
            if directory.is_empty() { filename.to_string() } else { format!("{directory}/{filename}") }
        };
        if package.rels_checked(&owner)?.list.iter().any(|r| !r.external && r.target == chart_part && rel_is(&r.kind, &format!("{XLSX_OFFICE}chart")))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn validate_changes(mapping: &ChartMapping, changes: &ChartDataChanges) -> Result<(), DocxError> {
    if changes.series.len() != mapping.series.len() {
        return Err(transaction_error("change set must contain exactly one entry per chart series"));
    }
    let mut any = false;
    validate_axis_change(mapping.horizontal_axis.as_ref(), changes.horizontal_axis.as_ref(), "horizontal")?;
    validate_axis_change(mapping.vertical_axis.as_ref(), changes.vertical_axis.as_ref(), "vertical")?;
    any |= changes.horizontal_axis.as_ref().is_some_and(axis_change_nonempty);
    any |= changes.vertical_axis.as_ref().is_some_and(axis_change_nonempty);
    if let Some(position) = &changes.legend_position {
        any = true;
        if !matches!(position.as_str(), "l" | "r" | "t" | "b") {
            return Err(transaction_error("legend position must be left, right, top, or bottom"));
        }
        if mapping.legend_position.is_none() {
            return Err(transaction_error("this chart has no explicitly supported legend position to edit"));
        }
    }
    for (value, existing, label) in [
        (changes.chart_title.as_ref(), mapping.chart_title.as_ref(), "chart title"),
        (changes.horizontal_axis_title.as_ref(), mapping.horizontal_axis_title.as_ref(), "horizontal axis title"),
        (changes.vertical_axis_title.as_ref(), mapping.vertical_axis_title.as_ref(), "vertical axis title"),
    ] {
        if let Some(value) = value {
            any = true;
            validate_label(value)?;
            if value.is_empty() {
                return Err(transaction_error(format!("{label} cannot be blank; adding or removing titles is not supported")));
            }
            if existing.is_none() {
                return Err(transaction_error(format!("{label} is absent or uses an unsupported OOXML structure")));
            }
        }
    }
    for (source, change) in mapping.series.iter().zip(&changes.series) {
        let scatter = matches!(&source.data, SeriesDataMapping::Scatter { .. });
        if let Some(color) = &change.color {
            any = true;
            if !source.formatting.color_editable || !valid_rgb_hex(color) {
                return Err(transaction_error("series color is unsupported or is not an editable six-digit RGB value"));
            }
        }
        if let Some(width) = change.line_width {
            any = true;
            if !source.formatting.line_width_editable || !width.is_finite() || !(0.0..=20.0).contains(&width) {
                return Err(transaction_error("series line width is unsupported or outside the safe 0–20 pt range"));
            }
        }
        if let Some(symbol) = &change.marker_symbol {
            any = true;
            if !source.formatting.marker_symbol_editable || !supported_marker_symbol(symbol) {
                return Err(transaction_error("scatter marker symbol is unsupported or has no safe explicit target"));
            }
        }
        if let Some(size) = change.marker_size {
            any = true;
            if !source.formatting.marker_size_editable || !(2..=72).contains(&size) {
                return Err(transaction_error("scatter marker size is unsupported or outside the safe 2–72 pt range"));
            }
        }
        if let Some(color) = &change.marker_color {
            any = true;
            if !source.formatting.marker_color_editable || !valid_rgb_hex(color) {
                return Err(transaction_error("scatter marker color has no safe explicit RGB target"));
            }
        }
        if let Some(labels) = &change.data_labels {
            any |= labels.show_value.is_some() || labels.show_category.is_some() || labels.show_series_name.is_some() || labels.position.is_some();
            let Some(snapshot) = source.formatting.data_labels.as_ref().filter(|snapshot| snapshot.editable) else {
                return Err(transaction_error("this series' data labels contain unsupported OOXML or lack explicit editable properties"));
            };
            for (requested, existing, label) in [
                (labels.show_value, snapshot.show_value, "numeric-value"),
                (labels.show_category, snapshot.show_category, "category-name"),
                (labels.show_series_name, snapshot.show_series_name, "series-name"),
            ] {
                if requested.is_some() && existing.is_none() {
                    return Err(transaction_error(format!("the {label} label setting is not explicitly present and cannot be changed safely")));
                }
            }
            if let Some(position) = labels.position.as_deref() {
                let supported: &[&str] = match mapping.chart_kind.as_str() {
                    "bar" | "column" => &["ctr", "inBase", "inEnd", "outEnd"],
                    "line" | "area" | "scatter" => &["ctr", "t", "b", "l", "r"],
                    "pie" | "doughnut" => &["ctr", "inEnd"],
                    _ => &[],
                };
                if snapshot.position.is_none() || !supported.contains(&position) {
                    return Err(transaction_error("data-label position is absent or unsupported for this chart family"));
                }
            }
        }
        if let Some(name) = &change.name {
            any = true;
            validate_label(name)?;
            if !matches!(&source.name.source, CellSource::SharedString(_) | CellSource::InlineString | CellSource::StringValue) {
                return Err(transaction_error("series name is not stored in an editable string cell"));
            }
        }
        if let Some(categories) = &change.categories {
            any = true;
            let SeriesDataMapping::CategoryValues { categories: source_categories, .. } = &source.data else {
                return Err(transaction_error("scatter charts use numeric X/Y values and do not accept category labels"));
            };
            if categories.len() != source_categories.len() {
                return Err(transaction_error("category edit length must match the validated chart range"));
            }
            for label in categories {
                validate_label(label)?;
            }
            if source_categories
                .iter()
                .any(|cell| !matches!(&cell.source, CellSource::SharedString(_) | CellSource::InlineString | CellSource::StringValue))
            {
                return Err(transaction_error("category range contains a non-string cell"));
            }
        }
        if let Some(x_values) = &change.x_values {
            any = true;
            let SeriesDataMapping::Scatter { x_values: source_x, .. } = &source.data else {
                return Err(transaction_error("X values can only be edited for validated scatter charts"));
            };
            if x_values.len() != source_x.len() {
                return Err(transaction_error("scatter X value count must match the validated source range"));
            }
            if x_values.iter().any(|value| !value.is_finite()) {
                return Err(transaction_error("scatter X values must be finite numbers"));
            }
        }
        if let Some(values) = &change.values {
            any = true;
            let source_values = match &source.data {
                SeriesDataMapping::CategoryValues { values, .. } => values,
                SeriesDataMapping::Scatter { y_values, .. } => y_values,
            };
            if values.len() != source_values.len() {
                return Err(transaction_error("numeric edit length must match the validated chart range"));
            }
            if values.iter().any(|value| !value.is_finite()) {
                return Err(transaction_error("chart values must be finite numbers"));
            }
        }
        if !scatter && change.x_values.is_some() {
            return Err(transaction_error("X values can only be edited for validated scatter charts"));
        }
    }
    if !any {
        return Err(transaction_error("change set contains no edits"));
    }
    Ok(())
}

fn axis_change_nonempty(change: &AxisChanges) -> bool {
    change.minimum.is_some() || change.maximum.is_some() || change.number_format.is_some()
}

fn valid_rgb_hex(color: &str) -> bool {
    color.len() == 6 && color.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_axis_change(current: Option<&ChartAxisSnapshot>, change: Option<&AxisChanges>, label: &str) -> Result<(), DocxError> {
    let Some(change) = change else { return Ok(()) };
    if !axis_change_nonempty(change) {
        return Err(transaction_error(format!("{label} axis change is empty")));
    }
    let current = current.ok_or_else(|| transaction_error(format!("{label} axis is not uniquely mapped")))?;
    if !current.editable {
        return Err(transaction_error(current.diagnostic.clone().unwrap_or_else(|| format!("{label} axis is read-only"))));
    }
    let bound = |change: &Option<AxisBoundChange>, original: Option<f64>| match change {
        Some(AxisBoundChange::Auto) => Ok(None),
        Some(AxisBoundChange::Manual { value }) if value.is_finite() => Ok(Some(*value)),
        Some(AxisBoundChange::Manual { .. }) => Err(transaction_error("axis bounds must be finite numbers")),
        None => Ok(original),
    };
    let minimum = bound(&change.minimum, current.minimum)?;
    let maximum = bound(&change.maximum, current.maximum)?;
    if minimum.zip(maximum).is_some_and(|(minimum, maximum)| minimum >= maximum || !(maximum - minimum).is_finite()) {
        return Err(transaction_error("axis minimum must be less than its maximum"));
    }
    if let Some(format) = &change.number_format
        && (!current.number_format_editable || !safe_axis_format(format))
    {
        return Err(transaction_error("axis number format is unsupported or preserved as read-only"));
    }
    Ok(())
}

fn apply_axis_snapshot(current: Option<&ChartAxisSnapshot>, change: &AxisChanges) -> Result<Option<ChartAxisSnapshot>, DocxError> {
    let mut current = current.cloned().ok_or_else(|| transaction_error("axis mapping is missing"))?;
    if let Some(bound) = &change.minimum {
        current.minimum = match bound {
            AxisBoundChange::Auto => None,
            AxisBoundChange::Manual { value } => Some(*value),
        };
    }
    if let Some(bound) = &change.maximum {
        current.maximum = match bound {
            AxisBoundChange::Auto => None,
            AxisBoundChange::Manual { value } => Some(*value),
        };
    }
    if let Some(format) = &change.number_format {
        current.number_format = Some(format.clone());
    }
    Ok(Some(current))
}

fn validate_label(value: &str) -> Result<(), DocxError> {
    if value.len() > MAX_LABEL_BYTES
        || value.trim() != value
        || value.chars().any(|c| !(c == '\t' || c == '\n' || c == '\r' || c >= '\u{20}') || matches!(c, '\u{fffe}' | '\u{ffff}'))
    {
        return Err(transaction_error("labels must be XML-safe, at most 4096 bytes, and have no leading or trailing whitespace"));
    }
    Ok(())
}

fn build_cell_edits(
    mapping: &ChartMapping,
    changes: &ChartDataChanges,
    expected: &mut ChartMapping,
) -> Result<BTreeMap<TextTarget, CellEdit>, DocxError> {
    let mut edits = BTreeMap::new();
    for (series_index, (series, change)) in mapping.series.iter().zip(&changes.series).enumerate() {
        let expected_series = expected.series.get_mut(series_index).ok_or_else(|| transaction_error("series mapping changed during transaction"))?;
        if let Some(value) = &change.name {
            add_text_edit(&mut edits, &series.name, value)?;
            expected_series.name.value = value.clone();
        }
        if let (Some(values), SeriesDataMapping::CategoryValues { categories, .. }) = (&change.categories, &series.data)
            && let SeriesDataMapping::CategoryValues { categories: expected_categories, .. } = &mut expected_series.data
        {
            for ((cell, value), expected_cell) in categories.iter().zip(values).zip(expected_categories) {
                add_text_edit(&mut edits, cell, value)?;
                expected_cell.value = value.clone();
            }
        }
        if let (Some(values), SeriesDataMapping::Scatter { x_values, .. }) = (&change.x_values, &series.data)
            && let SeriesDataMapping::Scatter { x_values: expected_x, .. } = &mut expected_series.data
        {
            for ((cell, value), expected_cell) in x_values.iter().zip(values).zip(expected_x) {
                add_value_edit(&mut edits, cell, *value)?;
                expected_cell.value = *value;
            }
        }
        if let Some(values) = &change.values {
            let (cells, expected_cells) = match (&series.data, &mut expected_series.data) {
                (SeriesDataMapping::CategoryValues { values: cells, .. }, SeriesDataMapping::CategoryValues { values: expected, .. }) => {
                    (cells, expected)
                }
                (SeriesDataMapping::Scatter { y_values: cells, .. }, SeriesDataMapping::Scatter { y_values: expected, .. }) => (cells, expected),
                _ => return Err(transaction_error("chart series mapping type changed during edit")),
            };
            for ((cell, value), expected_cell) in cells.iter().zip(values).zip(expected_cells) {
                add_value_edit(&mut edits, cell, *value)?;
                expected_cell.value = *value;
            }
        }
    }
    Ok(edits)
}

fn add_text_edit(edits: &mut BTreeMap<TextTarget, CellEdit>, cell: &MappedCell<String>, value: &str) -> Result<(), DocxError> {
    insert_cell_edit(
        edits,
        TextTarget { worksheet_part: cell.worksheet_part.clone(), address: cell.address.clone() },
        CellEdit::Text(value.to_string()),
    )
}

fn add_value_edit(edits: &mut BTreeMap<TextTarget, CellEdit>, cell: &MappedCell<f64>, value: f64) -> Result<(), DocxError> {
    insert_cell_edit(edits, TextTarget { worksheet_part: cell.worksheet_part.clone(), address: cell.address.clone() }, CellEdit::Number(value))
}

fn insert_cell_edit(edits: &mut BTreeMap<TextTarget, CellEdit>, target: TextTarget, edit: CellEdit) -> Result<(), DocxError> {
    if let Some(previous) = edits.get(&target) {
        if previous != &edit {
            return Err(transaction_error(format!("conflicting duplicate update for {}!{}", target.worksheet_part, target.address)));
        }
        return Ok(());
    }
    edits.insert(target, edit);
    Ok(())
}

fn update_chart_caches(chart: &[u8], mapping: &ChartMapping, changes: &ChartDataChanges) -> Result<Vec<u8>, DocxError> {
    let nodes = preserved_xml::inspect(chart)?;
    if nodes.is_empty() {
        return Err(transaction_error("chart XML has no root"));
    }
    let chart_kinds: Vec<_> = ["barChart", "lineChart", "areaChart", "pieChart", "doughnutChart", "scatterChart"]
        .into_iter()
        .flat_map(|name| descendants(&nodes, 0, CHART_NS, name))
        .collect();
    if chart_kinds.len() != 1 {
        return Err(transaction_error("chart no longer has one supported cache structure"));
    }
    let chart_series = child_indices(&nodes, *chart_kinds.first().ok_or_else(|| transaction_error("chart plot area is missing"))?, CHART_NS, "ser");
    if chart_series.len() != changes.series.len() {
        return Err(transaction_error("chart series changed after mapping"));
    }
    let mut splices = Vec::new();
    if let Some(position) = &changes.legend_position {
        let legends = descendants(&nodes, 0, CHART_NS, "legend");
        if legends.len() != 1 {
            return Err(transaction_error("chart does not contain exactly one editable legend"));
        }
        let legend = *legends.first().ok_or_else(|| transaction_error("chart legend is missing"))?;
        let positions = child_indices(&nodes, legend, CHART_NS, "legendPos");
        if positions.len() != 1 {
            return Err(transaction_error("legend position must already be explicitly present"));
        }
        let position_node = *positions.first().ok_or_else(|| transaction_error("legend position is missing"))?;
        add_attribute_value_splice(chart, &nodes, position_node, "val", position, &mut splices)?;
    }
    let title_nodes =
        inspect_chart_title_nodes(chart, &mapping.chart_kind).map_err(|_| transaction_error("chart title or axis title structure is unsupported"))?;
    for (requested, title_node, mapped_title, description) in [
        (changes.chart_title.as_ref(), title_nodes.chart, mapping.chart_title.as_ref(), "chart title"),
        (changes.horizontal_axis_title.as_ref(), title_nodes.horizontal_axis, mapping.horizontal_axis_title.as_ref(), "horizontal axis title"),
        (changes.vertical_axis_title.as_ref(), title_nodes.vertical_axis, mapping.vertical_axis_title.as_ref(), "vertical axis title"),
    ] {
        if let Some(value) = requested {
            let title_node = title_node.ok_or_else(|| transaction_error(format!("{description} is missing or has an unsupported structure")))?;
            let (text_node, current) = editable_title_text(&title_nodes.nodes, title_node)
                .ok_or_else(|| transaction_error(format!("{description} is not a supported single-run rich-text title")))?;
            if mapped_title != Some(&current) {
                return Err(transaction_error(format!("stale {description}: its contents changed after mapping")));
            }
            add_text_node_splice(chart, &title_nodes.nodes, text_node, value, &mut splices)?;
        }
    }
    for (series_index, (series_node, change)) in chart_series.iter().zip(&changes.series).enumerate() {
        let mut cache_edits = vec![("tx", "strCache", change.name.as_ref().map(|value| vec![value.clone()]))];
        match mapping.series.get(series_index).map(|series| &series.data) {
            Some(SeriesDataMapping::CategoryValues { .. }) => {
                cache_edits.push(("cat", "strCache", change.categories.clone()));
                cache_edits.push(("val", "numCache", change.values.as_ref().map(|values| values.iter().map(ToString::to_string).collect())));
            }
            Some(SeriesDataMapping::Scatter { .. }) => {
                cache_edits.push(("xVal", "numCache", change.x_values.as_ref().map(|values| values.iter().map(ToString::to_string).collect())));
                cache_edits.push(("yVal", "numCache", change.values.as_ref().map(|values| values.iter().map(ToString::to_string).collect())));
            }
            None => return Err(transaction_error("chart series mapping is missing")),
        }
        for (container_name, cache_name, values) in cache_edits {
            let Some(values) = values else { continue };
            let container = child_index(&nodes, *series_node, CHART_NS, container_name)
                .ok_or_else(|| transaction_error(format!("series {series_index} is missing {container_name}")))?;
            let references = child_indices_any(&nodes, container, CHART_NS, &["strRef", "numRef"]);
            if references.len() != 1 {
                return Err(transaction_error("chart source has multiple or unsupported references"));
            }
            let reference = *references.first().ok_or_else(|| transaction_error("chart source reference is missing"))?;
            let caches = child_indices(&nodes, reference, CHART_NS, cache_name);
            if caches.len() != 1 {
                return Err(transaction_error("chart source has multiple or missing caches"));
            }
            let cache = *caches.first().ok_or_else(|| transaction_error("chart source cache is missing"))?;
            let points = child_indices(&nodes, cache, CHART_NS, "pt");
            if points.len() != values.len() {
                return Err(transaction_error("chart cache length changed after mapping"));
            }
            for (point, value) in points.into_iter().zip(values) {
                let point_index = nodes
                    .get(point)
                    .and_then(|node| node.attrs.get(&qualified("", "idx")))
                    .and_then(|value| value.parse::<usize>().ok())
                    .ok_or_else(|| transaction_error("chart cache point index is malformed"))?;
                let expected_index = splices_in_cache_index(&nodes, cache, point)?;
                if expected_index != point_index {
                    return Err(transaction_error("chart cache points are not in their expected order"));
                }
                let value_node = child_index(&nodes, point, CHART_NS, "v").ok_or_else(|| transaction_error("chart cache point has no value"))?;
                add_text_node_splice(chart, &nodes, value_node, &value, &mut splices)?;
            }
        }
    }
    apply_splices(chart, splices)
}

fn apply_series_format_changes(chart: &[u8], mapping: &ChartMapping, changes: &ChartDataChanges) -> Result<Vec<u8>, DocxError> {
    let mut current = chart.to_vec();
    for (series_index, change) in changes.series.iter().enumerate() {
        if let Some(color) = &change.color {
            let nodes = preserved_xml::inspect(&current)?;
            let target = series_color_target_node(&nodes, &mapping.chart_kind, series_index)?
                .ok_or_else(|| transaction_error("series color no longer has a safe explicit RGB target"))?;
            current = set_xml_attribute(&current, &nodes, target, "val", &color.to_ascii_uppercase())?;
        }
        if let Some(width) = change.line_width {
            let nodes = preserved_xml::inspect(&current)?;
            let line = series_line_node(&nodes, &mapping.chart_kind, series_index)?
                .ok_or_else(|| transaction_error("series line width no longer has a safe target"))?;
            let emu_width = (width * 12_700.0).round();
            if !emu_width.is_finite() || !(0.0..=254_000.0).contains(&emu_width) {
                return Err(transaction_error("series line width cannot be represented safely"));
            }
            current = set_xml_attribute(&current, &nodes, line, "w", &format!("{emu_width:.0}"))?;
        }
        if let Some(symbol) = &change.marker_symbol {
            let nodes = preserved_xml::inspect(&current)?;
            let target = marker_attribute_node(&nodes, series_index, "symbol")?
                .ok_or_else(|| transaction_error("scatter marker symbol no longer has a safe explicit target"))?;
            current = set_xml_attribute(&current, &nodes, target, "val", symbol)?;
        }
        if let Some(size) = change.marker_size {
            let nodes = preserved_xml::inspect(&current)?;
            let target = marker_attribute_node(&nodes, series_index, "size")?
                .ok_or_else(|| transaction_error("scatter marker size no longer has a safe explicit target"))?;
            current = set_xml_attribute(&current, &nodes, target, "val", &size.to_string())?;
        }
        if let Some(color) = &change.marker_color {
            let nodes = preserved_xml::inspect(&current)?;
            let target = marker_color_target_node(&nodes, series_index)?
                .ok_or_else(|| transaction_error("scatter marker color no longer has a safe explicit RGB target"))?;
            current = set_xml_attribute(&current, &nodes, target, "val", &color.to_ascii_uppercase())?;
        }
    }
    Ok(current)
}

fn apply_data_label_changes(chart: &[u8], mapping: &ChartMapping, changes: &ChartDataChanges) -> Result<Vec<u8>, DocxError> {
    let mut current = chart.to_vec();
    for (series_index, change) in changes.series.iter().enumerate() {
        let Some(labels) = change.data_labels.as_ref() else { continue };
        for (field, value) in [
            ("showVal", labels.show_value.map(|value| if value { "1" } else { "0" })),
            ("showCatName", labels.show_category.map(|value| if value { "1" } else { "0" })),
            ("showSerName", labels.show_series_name.map(|value| if value { "1" } else { "0" })),
            ("dLblPos", labels.position.as_deref()),
        ] {
            let Some(value) = value else { continue };
            let nodes = preserved_xml::inspect(&current)?;
            let series = chart_series_nodes(&nodes, &mapping.chart_kind)?;
            let index = *series.get(series_index).ok_or_else(|| transaction_error("series index is out of range"))?;
            let blocks = child_indices(&nodes, index, CHART_NS, "dLbls");
            if blocks.len() != 1 {
                return Err(transaction_error("data-label block changed after validation"));
            }
            let block = *blocks.first().ok_or_else(|| transaction_error("data-label block is missing"))?;
            let local = if field == "dLblPos" { "dLblPos" } else { field };
            let targets = child_indices(&nodes, block, CHART_NS, local);
            if targets.len() != 1 {
                return Err(transaction_error(format!("data-label property `{field}` no longer has one explicit target")));
            }
            let target = *targets.first().ok_or_else(|| transaction_error("data-label property target is missing"))?;
            if !nodes
                .get(target)
                .is_some_and(|node| node.children.is_empty() && node.text.trim().is_empty() && node.attrs.contains_key(&qualified("", "val")))
            {
                return Err(transaction_error(format!("data-label property `{field}` has unsupported formatting")));
            }
            current = set_xml_attribute(&current, &nodes, target, "val", value)?;
        }
    }
    Ok(current)
}

fn marker_attribute_node(nodes: &[Element], series_index: usize, child: &str) -> Result<Option<usize>, DocxError> {
    let series = chart_series_nodes(nodes, "scatter")?;
    let Some(series) = series.get(series_index) else { return Err(transaction_error("series index is out of range")) };
    let markers = child_indices(nodes, *series, CHART_NS, "marker");
    if markers.len() != 1 {
        return Ok(None);
    }
    let marker = *markers.first().ok_or_else(|| transaction_error("scatter marker is missing"))?;
    let target = child_index(nodes, marker, CHART_NS, child);
    Ok(target.filter(|target| nodes.get(*target).is_some_and(|node| node.children.is_empty() && node.attrs.contains_key(&qualified("", "val")))))
}

fn marker_color_target_node(nodes: &[Element], series_index: usize) -> Result<Option<usize>, DocxError> {
    let series = chart_series_nodes(nodes, "scatter")?;
    let Some(series) = series.get(series_index) else { return Err(transaction_error("series index is out of range")) };
    let markers = child_indices(nodes, *series, CHART_NS, "marker");
    if markers.len() != 1 {
        return Ok(None);
    }
    let marker = *markers.first().ok_or_else(|| transaction_error("scatter marker is missing"))?;
    let properties = child_indices(nodes, marker, CHART_NS, "spPr");
    if properties.len() != 1 {
        return Ok(None);
    }
    let Some(properties) = properties.first().copied() else { return Ok(None) };
    let fills = child_indices_any(nodes, properties, DRAWING_NS, &["solidFill", "gradFill", "pattFill", "noFill", "blipFill"]);
    if fills.len() != 1 {
        return Ok(None);
    }
    let fill = *fills.first().ok_or_else(|| transaction_error("scatter marker fill is missing"))?;
    if !nodes.get(fill).is_some_and(|node| node.is(DRAWING_NS, "solidFill")) {
        return Ok(None);
    }
    Ok(simple_srgb_node(nodes, fill))
}

fn chart_series_nodes(nodes: &[Element], chart_kind: &str) -> Result<Vec<usize>, DocxError> {
    let local = match chart_kind {
        "bar" | "column" => "barChart",
        "line" => "lineChart",
        "area" => "areaChart",
        "pie" => "pieChart",
        "doughnut" => "doughnutChart",
        "scatter" => "scatterChart",
        _ => return Err(transaction_error("chart family is unsupported for series formatting")),
    };
    let chart_types = descendants(nodes, 0, CHART_NS, local);
    if chart_types.len() != 1 {
        return Err(transaction_error("chart formatting target is ambiguous"));
    }
    let chart_type = *chart_types.first().ok_or_else(|| transaction_error("chart formatting target is missing"))?;
    Ok(child_indices(nodes, chart_type, CHART_NS, "ser"))
}

fn simple_srgb_node(nodes: &[Element], fill: usize) -> Option<usize> {
    if nodes.get(fill)?.children.len() != 1 {
        return None;
    }
    let colors = child_indices(nodes, fill, DRAWING_NS, "srgbClr");
    if colors.len() != 1 || !child_indices_any(nodes, fill, DRAWING_NS, &["schemeClr", "sysClr", "prstClr", "scrgbClr"]).is_empty() {
        return None;
    }
    let color = *colors.first()?;
    let node = nodes.get(color)?;
    if !node.children.is_empty() || !node.attrs.contains_key(&qualified("", "val")) {
        return None;
    }
    let value = node.attrs.get(&qualified("", "val"))?;
    (valid_rgb_hex(value)
        && child_indices_any(nodes, color, DRAWING_NS, &["tint", "shade", "lumMod", "lumOff", "alpha", "alphaMod", "alphaOff"]).is_empty())
    .then_some(color)
}

fn fill_color_node(nodes: &[Element], parent: usize) -> Option<Option<usize>> {
    let fills = child_indices_any(nodes, parent, DRAWING_NS, &["solidFill", "gradFill", "pattFill", "noFill", "blipFill"]);
    if fills.is_empty() {
        return Some(None);
    }
    if fills.len() != 1 {
        return None;
    }
    let fill = *fills.first()?;
    if !nodes.get(fill)?.is(DRAWING_NS, "solidFill") {
        return None;
    }
    simple_srgb_node(nodes, fill).map(Some)
}

fn series_color_target_node(nodes: &[Element], chart_kind: &str, series_index: usize) -> Result<Option<usize>, DocxError> {
    let series_nodes = chart_series_nodes(nodes, chart_kind)?;
    let series = *series_nodes.get(series_index).ok_or_else(|| transaction_error("series index is out of range"))?;
    if matches!(chart_kind, "pie" | "doughnut") {
        let chart_local = if chart_kind == "pie" { "pieChart" } else { "doughnutChart" };
        let chart_types = descendants(nodes, 0, CHART_NS, chart_local);
        let varies = chart_types.first().and_then(|chart| child_index(nodes, *chart, CHART_NS, "varyColors")).is_none_or(|index| {
            nodes.get(index).and_then(|node| node.attrs.get(&qualified("", "val"))).is_none_or(|value| !matches!(value.as_str(), "0" | "false"))
        });
        if varies {
            return Ok(None);
        }
    }
    let Some(properties) = child_index(nodes, series, CHART_NS, "spPr") else {
        if chart_kind == "scatter"
            && let Some(marker) = child_index(nodes, series, CHART_NS, "marker")
            && let Some(marker_properties) = child_index(nodes, marker, CHART_NS, "spPr")
        {
            return Ok(fill_color_node(nodes, marker_properties).flatten());
        }
        return Ok(None);
    };
    if matches!(chart_kind, "line" | "scatter") {
        let competing_fill = child_indices_any(nodes, properties, DRAWING_NS, &["solidFill", "gradFill", "pattFill", "blipFill"]);
        if !competing_fill.is_empty() {
            return Ok(None);
        }
        if let Some(line) = child_index(nodes, properties, DRAWING_NS, "ln") {
            match fill_color_node(nodes, line) {
                Some(Some(target)) => return Ok(Some(target)),
                Some(None) => {}
                None => return Ok(None),
            }
        }
        if chart_kind == "scatter"
            && let Some(marker) = child_index(nodes, series, CHART_NS, "marker")
            && let Some(marker_properties) = child_index(nodes, marker, CHART_NS, "spPr")
        {
            return Ok(fill_color_node(nodes, marker_properties).flatten());
        }
        return Ok(None);
    }
    Ok(fill_color_node(nodes, properties).flatten())
}

fn series_line_node(nodes: &[Element], chart_kind: &str, series_index: usize) -> Result<Option<usize>, DocxError> {
    if !matches!(chart_kind, "line" | "scatter") {
        return Ok(None);
    }
    let series_nodes = chart_series_nodes(nodes, chart_kind)?;
    let series = *series_nodes.get(series_index).ok_or_else(|| transaction_error("series index is out of range"))?;
    let Some(properties) = child_index(nodes, series, CHART_NS, "spPr") else { return Ok(None) };
    let lines = child_indices(nodes, properties, DRAWING_NS, "ln");
    if lines.len() != 1 {
        return Ok(None);
    }
    let line = *lines.first().ok_or_else(|| transaction_error("series line target is missing"))?;
    if !nodes.get(line).is_some_and(|node| node.attrs.contains_key(&qualified("", "w"))) {
        return Ok(None);
    }
    Ok(Some(line))
}

fn apply_axis_changes(chart: &[u8], mapping: &ChartMapping, changes: &ChartDataChanges) -> Result<Vec<u8>, DocxError> {
    let mut output = chart.to_vec();
    for (position, axis_change) in [("horizontal", changes.horizontal_axis.as_ref()), ("vertical", changes.vertical_axis.as_ref())] {
        let Some(change) = axis_change else { continue };
        output = edit_axis_property(&output, mapping, position, change)?;
    }
    Ok(output)
}

fn edit_axis_property(chart: &[u8], mapping: &ChartMapping, position: &str, change: &AxisChanges) -> Result<Vec<u8>, DocxError> {
    let mut current = chart.to_vec();
    for (name, requested) in [("min", change.minimum.as_ref()), ("max", change.maximum.as_ref())] {
        let Some(requested) = requested else { continue };
        let nodes = preserved_xml::inspect(&current)?;
        let axis = axis_node_for_position(&nodes, &mapping.chart_kind, position)?;
        current = set_axis_bound(&current, &nodes, axis, name, requested)?;
    }
    if let Some(format) = &change.number_format {
        current = set_axis_number_format(&current, mapping, position, format)?;
    }
    Ok(current)
}

fn axis_node_for_position(nodes: &[Element], chart_kind: &str, position: &str) -> Result<usize, DocxError> {
    let chart_types: Vec<_> =
        ["barChart", "lineChart", "areaChart", "scatterChart"].into_iter().flat_map(|local| descendants(nodes, 0, CHART_NS, local)).collect();
    if chart_types.len() != 1 {
        return Err(transaction_error("chart axis structure is ambiguous"));
    }
    let chart_type = *chart_types.first().ok_or_else(|| transaction_error("chart axis structure is missing"))?;
    let chart_ids = child_indices(nodes, chart_type, CHART_NS, "axId");
    let ids: Vec<_> = chart_ids.iter().filter_map(|index| nodes.get(*index)?.attrs.get(&qualified("", "val")).cloned()).collect();
    if chart_ids.len() != 2 || ids.len() != 2 || ids.first() == ids.get(1) {
        return Err(transaction_error("secondary or ambiguous chart axes are read-only"));
    }
    let plot = nodes.iter().position(|node| node.children.contains(&chart_type)).ok_or_else(|| transaction_error("chart plot area is missing"))?;
    let val_axes: Vec<_> = child_indices(nodes, plot, CHART_NS, "valAx")
        .into_iter()
        .filter(|index| {
            nodes
                .get(*index)
                .and_then(|_node| child_index(nodes, *index, CHART_NS, "axId"))
                .and_then(|id_node| nodes.get(id_node))
                .and_then(|id_node| id_node.attrs.get(&qualified("", "val")))
                .is_some_and(|id| ids.contains(id))
        })
        .filter(|index| {
            nodes
                .get(*index)
                .and_then(|_node| child_index(nodes, *index, CHART_NS, "axPos"))
                .and_then(|pos| nodes.get(pos))
                .and_then(|pos| pos.attrs.get(&qualified("", "val")))
                .is_some_and(|value| if position == "horizontal" { matches!(value.as_str(), "b" | "t") } else { matches!(value.as_str(), "l" | "r") })
        })
        .collect();
    if val_axes.len() != 1 {
        return Err(transaction_error(format!("{position} numeric axis is not uniquely mapped")));
    }
    let axis = *val_axes.first().ok_or_else(|| transaction_error("numeric axis is missing"))?;
    let actual_position = child_index(nodes, axis, CHART_NS, "axPos")
        .and_then(|index| nodes.get(index))
        .and_then(|node| node.attrs.get(&qualified("", "val")))
        .ok_or_else(|| transaction_error("numeric axis position is malformed"))?;
    if chart_kind == "scatter" && ((position == "horizontal") != matches!(actual_position.as_str(), "b" | "t")) {
        return Err(transaction_error("scatter X/Y axis mapping is inconsistent"));
    }
    Ok(axis)
}

fn set_axis_bound(chart: &[u8], nodes: &[Element], axis: usize, name: &str, value: &AxisBoundChange) -> Result<Vec<u8>, DocxError> {
    let scaling = child_indices(nodes, axis, CHART_NS, "scaling");
    if scaling.len() > 1 {
        return Err(transaction_error("axis has duplicate scaling elements"));
    }
    let Some(scale) = scaling.first().copied() else {
        if matches!(value, AxisBoundChange::Auto) {
            return Ok(chart.to_vec());
        }
        let axis_node = nodes.get(axis).ok_or_else(|| transaction_error("numeric axis is missing"))?;
        let prefix = element_prefix(chart, axis_node)?;
        let numeric = match value {
            AxisBoundChange::Manual { value } if value.is_finite() => value.to_string(),
            _ => return Err(transaction_error("axis bound is invalid")),
        };
        let ax_id = child_index(nodes, axis, CHART_NS, "axId")
            .and_then(|index| nodes.get(index))
            .ok_or_else(|| transaction_error("axis identifier is missing"))?;
        return insert_at(chart, ax_id.span.end, format!("<{prefix}scaling><{prefix}{name} val=\"{numeric}\"/></{prefix}scaling>").as_bytes());
    };
    let property = child_indices(nodes, scale, CHART_NS, name);
    if property.len() > 1 {
        return Err(transaction_error(format!("axis has duplicate {name} bounds")));
    }
    match (property.first().copied(), value) {
        (Some(index), AxisBoundChange::Auto) => remove_element(chart, nodes, index),
        (Some(index), AxisBoundChange::Manual { value }) if value.is_finite() => set_xml_attribute(chart, nodes, index, "val", &value.to_string()),
        (None, AxisBoundChange::Auto) => Ok(chart.to_vec()),
        (None, AxisBoundChange::Manual { value }) if value.is_finite() => {
            let scale_node = nodes.get(scale).ok_or_else(|| transaction_error("axis scaling is missing"))?;
            let prefix = element_prefix(chart, nodes.get(axis).ok_or_else(|| transaction_error("axis is missing"))?)?;
            let markup = format!("<{prefix}{name} val=\"{}\"/>", value);
            let before = if name == "max" {
                child_indices(nodes, scale, CHART_NS, "min").first().copied()
            } else {
                child_indices(nodes, scale, CHART_NS, "extLst").first().copied()
            };
            insert_child(chart, scale_node, before.and_then(|index| nodes.get(index)), &markup)
        }
        _ => Err(transaction_error("axis bound must be finite")),
    }
}

fn set_axis_number_format(chart: &[u8], mapping: &ChartMapping, position: &str, format: &str) -> Result<Vec<u8>, DocxError> {
    if !safe_axis_format(format) {
        return Err(transaction_error("axis number format is unsupported"));
    }
    let nodes = preserved_xml::inspect(chart)?;
    let axis = axis_node_for_position(&nodes, &mapping.chart_kind, position)?;
    let formats = child_indices(&nodes, axis, CHART_NS, "numFmt");
    if formats.len() > 1 {
        return Err(transaction_error("axis has duplicate number formats"));
    }
    let Some(format_node) = formats.first().copied() else {
        let axis_node = nodes.get(axis).ok_or_else(|| transaction_error("numeric axis is missing"))?;
        let prefix = element_prefix(chart, axis_node)?;
        let markup = format!("<{prefix}numFmt formatCode=\"{format}\" sourceLinked=\"0\"/>");
        let later = [
            "majorTickMark",
            "minorTickMark",
            "tickLblPos",
            "spPr",
            "txPr",
            "crossAx",
            "crosses",
            "crossesAt",
            "crossBetween",
            "majorUnit",
            "minorUnit",
            "dispUnits",
            "extLst",
        ];
        let anchor = later.iter().find_map(|local| child_index(&nodes, axis, CHART_NS, local).and_then(|index| nodes.get(index)));
        let output = insert_child(chart, nodes.get(axis).ok_or_else(|| transaction_error("numeric axis is missing"))?, anchor, &markup)?;
        let parsed = preserved_xml::inspect(&output)?;
        let axis = axis_node_for_position(&parsed, &mapping.chart_kind, position)?;
        let format_node = child_index(&parsed, axis, CHART_NS, "numFmt").ok_or_else(|| transaction_error("number format insertion failed"))?;
        return set_xml_attribute(&output, &parsed, format_node, "formatCode", format);
    };
    let code = nodes.get(format_node).and_then(|node| node.attrs.get(&qualified("", "formatCode")));
    if code.is_none() {
        return Err(transaction_error("axis number format has no explicit formatCode attribute"));
    }
    let updated = set_xml_attribute(chart, &nodes, format_node, "formatCode", format)?;
    let reparsed = preserved_xml::inspect(&updated)?;
    // sourceLinked=0 is required so Word uses the explicit formatCode.
    let axis = axis_node_for_position(&reparsed, &mapping.chart_kind, position)?;
    let formats = child_indices(&reparsed, axis, CHART_NS, "numFmt");
    let format_node = *formats.first().ok_or_else(|| transaction_error("axis number format disappeared during editing"))?;
    let source_linked = reparsed.get(format_node).and_then(|node| node.attrs.get(&qualified("", "sourceLinked"))).map(String::as_str);
    if source_linked == Some("1") { set_xml_attribute(&updated, &reparsed, format_node, "sourceLinked", "0") } else { Ok(updated) }
}

fn element_prefix(source: &[u8], node: &Element) -> Result<String, DocxError> {
    let raw = source.get(node.span.clone()).ok_or_else(|| transaction_error("XML element range is invalid"))?;
    let name = raw
        .get(1..)
        .and_then(|tail| tail.split(|byte| byte.is_ascii_whitespace() || matches!(*byte, b'>' | b'/')).next())
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .ok_or_else(|| transaction_error("XML element name is malformed"))?;
    let (prefix, _) = name.split_once(':').ok_or_else(|| transaction_error("chart XML uses an unsupported unprefixed namespace"))?;
    Ok(format!("{prefix}:"))
}

fn insert_child(source: &[u8], parent: &Element, before: Option<&Element>, markup: &str) -> Result<Vec<u8>, DocxError> {
    if let Some(before) = before {
        return insert_at(source, before.span.start, markup.as_bytes());
    }
    if !parent.empty {
        return insert_at(source, parent.content.end, markup.as_bytes());
    }
    let raw = source.get(parent.span.clone()).ok_or_else(|| transaction_error("empty XML element range is invalid"))?;
    let close = raw.iter().rposition(|byte| *byte == b'>').ok_or_else(|| transaction_error("empty XML element is malformed"))?;
    let slash = raw
        .get(..close)
        .and_then(|bytes| bytes.iter().rposition(|byte| *byte == b'/'))
        .ok_or_else(|| transaction_error("empty XML element lacks a close slash"))?;
    let name_raw = raw
        .get(1..)
        .and_then(|tail| tail.split(|byte| byte.is_ascii_whitespace() || matches!(*byte, b'>' | b'/')).next())
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .ok_or_else(|| transaction_error("empty XML element name is malformed"))?;
    let mut replacement = raw.get(..slash).ok_or_else(|| transaction_error("empty XML tag range is invalid"))?.to_vec();
    replacement.extend_from_slice(b">");
    replacement.extend_from_slice(markup.as_bytes());
    replacement.extend_from_slice(format!("</{name_raw}>").as_bytes());
    let mut splices = vec![Splice { range: parent.span.clone(), replacement }];
    apply_splices(source, std::mem::take(&mut splices))
}

fn insert_at(source: &[u8], offset: usize, bytes: &[u8]) -> Result<Vec<u8>, DocxError> {
    if offset > source.len() {
        return Err(transaction_error("XML insertion point is out of bounds"));
    }
    apply_splices(source, vec![Splice { range: offset..offset, replacement: bytes.to_vec() }])
}

fn remove_element(source: &[u8], nodes: &[Element], index: usize) -> Result<Vec<u8>, DocxError> {
    let node = nodes.get(index).ok_or_else(|| transaction_error("axis XML element is missing"))?;
    apply_splices(source, vec![Splice { range: node.span.clone(), replacement: Vec::new() }])
}

fn set_xml_attribute(source: &[u8], nodes: &[Element], index: usize, attribute: &str, value: &str) -> Result<Vec<u8>, DocxError> {
    let node = nodes.get(index).ok_or_else(|| transaction_error("XML attribute node is missing"))?;
    if !node.attrs.contains_key(&qualified("", attribute)) {
        return Err(transaction_error(format!("XML attribute {attribute} is not explicitly present")));
    }
    let raw = source.get(node.span.clone()).ok_or_else(|| transaction_error("XML attribute range is invalid"))?;
    let needle = format!("{attribute}=\"");
    let start = raw
        .windows(needle.len())
        .position(|window| window == needle.as_bytes())
        .map(|offset| offset + needle.len())
        .ok_or_else(|| transaction_error(format!("XML attribute {attribute} is not explicitly present")))?;
    let end = raw
        .get(start..)
        .and_then(|tail| tail.iter().position(|byte| *byte == b'\"'))
        .map(|offset| start + offset)
        .ok_or_else(|| transaction_error("XML attribute value is unterminated"))?;
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '"' => escaped.push_str("&quot;"),
            c if c >= '\u{20}' => escaped.push(c),
            _ => return Err(transaction_error("XML attribute contains an invalid character")),
        }
    }
    let absolute_start = node.span.start.checked_add(start).ok_or_else(|| transaction_error("XML attribute offset overflow"))?;
    let absolute_end = node.span.start.checked_add(end).ok_or_else(|| transaction_error("XML attribute offset overflow"))?;
    apply_splices(source, vec![Splice { range: absolute_start..absolute_end, replacement: escaped.into_bytes() }])
}

fn splices_in_cache_index(nodes: &[Element], cache: usize, point: usize) -> Result<usize, DocxError> {
    child_indices(nodes, cache, CHART_NS, "pt")
        .iter()
        .position(|candidate| *candidate == point)
        .ok_or_else(|| transaction_error("chart cache point is not owned by its cache"))
}

fn child_indices(nodes: &[Element], parent: usize, namespace: &str, local: &str) -> Vec<usize> {
    nodes
        .get(parent)
        .map(|node| node.children.iter().filter_map(|index| nodes.get(*index).filter(|child| child.is(namespace, local)).map(|_| *index)).collect())
        .unwrap_or_default()
}

fn child_indices_any(nodes: &[Element], parent: usize, namespace: &str, locals: &[&str]) -> Vec<usize> {
    nodes
        .get(parent)
        .map(|node| {
            node.children
                .iter()
                .filter_map(|index| nodes.get(*index).filter(|child| locals.iter().any(|local| child.is(namespace, local))).map(|_| *index))
                .collect()
        })
        .unwrap_or_default()
}

fn child_index(nodes: &[Element], parent: usize, namespace: &str, local: &str) -> Option<usize> {
    child_indices(nodes, parent, namespace, local).into_iter().next()
}

fn descendants(nodes: &[Element], parent: usize, namespace: &str, local: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut pending = vec![parent];
    while let Some(index) = pending.pop() {
        if let Some(node) = nodes.get(index) {
            for child in node.children.iter().rev() {
                if let Some(element) = nodes.get(*child) {
                    if element.is(namespace, local) {
                        found.push(*child);
                    }
                    pending.push(*child);
                }
            }
        }
    }
    found
}

fn add_text_node_splice(source: &[u8], nodes: &[Element], index: usize, value: &str, splices: &mut Vec<Splice>) -> Result<(), DocxError> {
    let node = nodes.get(index).ok_or_else(|| transaction_error("XML text node is unavailable"))?;
    if node.empty || !node.children.is_empty() {
        return Err(transaction_error("mapped XML value is not a simple text node"));
    }
    let raw_content = source.get(node.content.clone()).ok_or_else(|| transaction_error("XML text content span is invalid"))?;
    if [b"<!--".as_slice(), b"<![CDATA[".as_slice(), b"<?".as_slice()]
        .iter()
        .any(|marker| raw_content.windows(marker.len()).any(|window| window == *marker))
    {
        return Err(transaction_error("comments, CDATA and processing instructions in mapped values are read-only"));
    }
    let encoded = escape_xml_text(value)?;
    if node.content.end > source.len() || node.content.start > node.content.end {
        return Err(transaction_error("XML text span is invalid"));
    }
    splices.push(Splice { range: node.content.clone(), replacement: encoded.into_bytes() });
    Ok(())
}

fn add_attribute_value_splice(
    source: &[u8],
    nodes: &[Element],
    index: usize,
    attribute: &str,
    value: &str,
    splices: &mut Vec<Splice>,
) -> Result<(), DocxError> {
    let source = match preserved_xml::utf8(source)? {
        std::borrow::Cow::Borrowed(bytes) => bytes,
        std::borrow::Cow::Owned(_) => return Err(transaction_error("editing UTF-16 chart XML is not supported")),
    };
    let node = nodes.get(index).ok_or_else(|| transaction_error("XML attribute node is unavailable"))?;
    let raw = source.get(node.span.clone()).ok_or_else(|| transaction_error("XML attribute element span is invalid"))?;
    let mut tag_end = None;
    let mut quote = None;
    for (offset, byte) in raw.iter().enumerate() {
        match (*byte, quote) {
            (b'\'' | b'"', None) => quote = Some(*byte),
            (b'\'' | b'"', Some(current)) if *byte == current => quote = None,
            (b'>', None) => {
                tag_end = Some(offset);
                break;
            }
            _ => {}
        }
    }
    let tag_end = tag_end.ok_or_else(|| transaction_error("XML attribute start tag is malformed"))?;
    let start = raw.get(..tag_end).ok_or_else(|| transaction_error("XML attribute start tag range is invalid"))?;
    let mut cursor = 1usize;
    while start.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    while start.get(cursor).is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(*byte, b'/' | b'>')) {
        cursor += 1;
    }
    let mut value_range = None;
    while cursor < start.len() {
        while start.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if start.get(cursor).is_none_or(|byte| matches!(*byte, b'/' | b'>')) {
            break;
        }
        let name_start = cursor;
        while start.get(cursor).is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(*byte, b'=' | b'/' | b'>')) {
            cursor += 1;
        }
        let name = std::str::from_utf8(start.get(name_start..cursor).ok_or_else(|| transaction_error("XML attribute name range is invalid"))?)
            .map_err(|_| transaction_error("XML attribute name is not UTF-8"))?;
        while start.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if start.get(cursor) != Some(&b'=') {
            return Err(transaction_error("XML attribute assignment is malformed"));
        }
        cursor += 1;
        while start.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let delimiter = *start.get(cursor).ok_or_else(|| transaction_error("XML attribute value is missing"))?;
        if !matches!(delimiter, b'\'' | b'"') {
            return Err(transaction_error("XML attribute value is unquoted"));
        }
        cursor += 1;
        let value_start = cursor;
        while start.get(cursor).is_some_and(|byte| *byte != delimiter) {
            cursor += 1;
        }
        if start.get(cursor) != Some(&delimiter) {
            return Err(transaction_error("XML attribute value is unterminated"));
        }
        if name == attribute && value_range.replace(value_start..cursor).is_some() {
            return Err(transaction_error("XML attribute is duplicated"));
        }
        cursor += 1;
    }
    let range = value_range.ok_or_else(|| transaction_error(format!("XML attribute {attribute} is missing")))?;
    if !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(transaction_error("XML attribute replacement contains unsupported characters"));
    }
    let absolute_start = node.span.start.checked_add(range.start).ok_or_else(|| transaction_error("XML attribute offset overflow"))?;
    let absolute_end = node.span.start.checked_add(range.end).ok_or_else(|| transaction_error("XML attribute offset overflow"))?;
    splices.push(Splice { range: absolute_start..absolute_end, replacement: value.as_bytes().to_vec() });
    Ok(())
}

fn escape_xml_text(value: &str) -> Result<String, DocxError> {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\r' => escaped.push_str("&#13;"),
            '\n' => escaped.push_str("&#10;"),
            '\t' => escaped.push_str("&#9;"),
            c if c >= '\u{20}' && !matches!(c, '\u{fffe}' | '\u{ffff}') => escaped.push(c),
            _ => return Err(transaction_error("label contains a forbidden XML character")),
        }
    }
    Ok(escaped)
}

fn apply_splices(source: &[u8], mut splices: Vec<Splice>) -> Result<Vec<u8>, DocxError> {
    splices.sort_by_key(|splice| splice.range.start);
    for adjacent in splices.windows(2) {
        if adjacent.first().is_some_and(|a| a.range.end > adjacent.get(1).map_or(0, |b| b.range.start)) {
            return Err(transaction_error("XML updates overlap"));
        }
    }
    let final_len = splices
        .iter()
        .try_fold(source.len(), |length, splice| {
            length.checked_sub(splice.range.len()).and_then(|length| length.checked_add(splice.replacement.len()))
        })
        .ok_or_else(|| transaction_error("XML updated size overflow"))?;
    if final_len > MAX_MUTATION_XML {
        return Err(transaction_error("updated XML exceeds the preservation limit"));
    }
    let mut output = Vec::with_capacity(final_len);
    let mut position = 0usize;
    for splice in splices {
        let untouched = source.get(position..splice.range.start).ok_or_else(|| transaction_error("XML splice range is out of bounds"))?;
        output.extend_from_slice(untouched);
        output.extend_from_slice(&splice.replacement);
        position = splice.range.end;
    }
    output.extend_from_slice(source.get(position..).ok_or_else(|| transaction_error("XML splice tail is out of bounds"))?);
    Ok(output)
}

fn apply_workbook_edits(workbook: &[u8], edits: &BTreeMap<TextTarget, CellEdit>) -> Result<Vec<u8>, DocxError> {
    let package = Package::open(workbook).map_err(|error| transaction_error(format!("embedded XLSX is unsafe: {error}")))?;
    let workbook_part = "xl/workbook.xml";
    let workbook_xml = package.get(workbook_part).ok_or_else(|| transaction_error("XLSX workbook.xml is missing"))?;
    let workbook_nodes = preserved_xml::inspect(workbook_xml)?;
    let rels = package.rels_checked(workbook_part)?;
    let sheets_node =
        descendants(&workbook_nodes, 0, SHEET_NS, "sheets").first().copied().ok_or_else(|| transaction_error("XLSX sheet list is missing"))?;
    let sheet_nodes = child_indices(&workbook_nodes, sheets_node, SHEET_NS, "sheet");
    let mut all_sheet_parts = BTreeSet::new();
    for sheet in sheet_nodes {
        let rel_id = workbook_nodes
            .get(sheet)
            .and_then(|node| node.attrs.get(&qualified(DOC_REL_NS, "id")))
            .ok_or_else(|| transaction_error("XLSX sheet relationship ID is missing"))?;
        let rel = rels
            .by_id(rel_id)
            .filter(|rel| !rel.external && rel_is(&rel.kind, &format!("{XLSX_OFFICE}worksheet")))
            .ok_or_else(|| transaction_error("XLSX sheet relationship is invalid"))?;
        all_sheet_parts.insert(rel.target.clone());
    }
    let shared_rel = rels.by_type(&format!("{XLSX_OFFICE}sharedStrings"));
    let shared_part = shared_rel.map(|rel| rel.target.clone());
    let mut splices: BTreeMap<String, Vec<Splice>> = BTreeMap::new();
    let mut shared_edits: BTreeMap<usize, String> = BTreeMap::new();
    let mut shared_target_uses: BTreeMap<usize, BTreeSet<TextTarget>> = BTreeMap::new();

    for (target, edit) in edits {
        let source_xml = package.get(&target.worksheet_part).ok_or_else(|| transaction_error("mapped worksheet is missing"))?;
        let nodes = preserved_xml::inspect(source_xml)?;
        let cell_node = find_cell(&nodes, &target.address).ok_or_else(|| transaction_error("mapped worksheet cell disappeared"))?;
        let cell = nodes.get(cell_node).ok_or_else(|| transaction_error("mapped worksheet cell is unavailable"))?;
        if !descendants(&nodes, cell_node, SHEET_NS, "f").is_empty() {
            return Err(transaction_error("formula-driven cells are read-only"));
        }
        match edit {
            CellEdit::Number(value) => {
                if cell.attrs.get(&qualified("", "t")).is_some_and(|kind| !matches!(kind.as_str(), "n")) {
                    return Err(transaction_error("numeric target cell changed type"));
                }
                let value_node =
                    child_index(&nodes, cell_node, SHEET_NS, "v").ok_or_else(|| transaction_error("numeric target cell has no value"))?;
                let canonical = value.to_string();
                add_text_node_splice(source_xml, &nodes, value_node, &canonical, splices.entry(target.worksheet_part.clone()).or_default())?;
            }
            CellEdit::Text(value) => match cell.attrs.get(&qualified("", "t")).map(String::as_str).unwrap_or("n") {
                "inlineStr" => {
                    let inline =
                        child_index(&nodes, cell_node, SHEET_NS, "is").ok_or_else(|| transaction_error("inline string cell changed structure"))?;
                    let text = child_index(&nodes, inline, SHEET_NS, "t")
                        .filter(|_| child_indices(&nodes, inline, SHEET_NS, "r").is_empty())
                        .ok_or_else(|| transaction_error("rich or unsupported inline strings are read-only"))?;
                    add_text_node_splice(source_xml, &nodes, text, value, splices.entry(target.worksheet_part.clone()).or_default())?;
                }
                "str" => {
                    let value_node = child_index(&nodes, cell_node, SHEET_NS, "v").ok_or_else(|| transaction_error("string cell has no value"))?;
                    add_text_node_splice(source_xml, &nodes, value_node, value, splices.entry(target.worksheet_part.clone()).or_default())?;
                }
                "s" => {
                    let shared_index = child_index(&nodes, cell_node, SHEET_NS, "v")
                        .and_then(|value_node| nodes.get(value_node))
                        .and_then(|value_node| value_node.text.parse::<usize>().ok())
                        .ok_or_else(|| transaction_error("shared-string cell index is invalid"))?;
                    if let Some(previous) = shared_edits.get(&shared_index)
                        && previous != value
                    {
                        return Err(transaction_error(format!("conflicting updates target shared-string item {shared_index}")));
                    }
                    shared_edits.insert(shared_index, value.clone());
                    shared_target_uses.entry(shared_index).or_default().insert(target.clone());
                }
                _ => return Err(transaction_error("mapped string cell has an unsupported storage type")),
            },
        }
    }

    if !shared_edits.is_empty() {
        let shared_part = shared_part.ok_or_else(|| transaction_error("shared-string relationship is missing"))?;
        let mapped_targets: BTreeSet<_> =
            edits.iter().filter_map(|(target, edit)| matches!(edit, CellEdit::Text(_)).then_some(target.clone())).collect();
        for part in &all_sheet_parts {
            let bytes = package.get(part).ok_or_else(|| transaction_error(format!("worksheet {part} is missing")))?;
            let nodes = preserved_xml::inspect(bytes)?;
            for cell_node in descendants(&nodes, 0, SHEET_NS, "c") {
                let Some(cell) = nodes.get(cell_node) else { continue };
                if cell.attrs.get(&qualified("", "t")).map(String::as_str) != Some("s") {
                    continue;
                }
                let Some(index) = child_index(&nodes, cell_node, SHEET_NS, "v")
                    .and_then(|value_node| nodes.get(value_node))
                    .and_then(|value_node| value_node.text.parse::<usize>().ok())
                else {
                    continue;
                };
                let Some(new_value) = shared_edits.get(&index) else { continue };
                let address = cell.attrs.get(&qualified("", "r")).cloned().ok_or_else(|| transaction_error("shared-string cell has no address"))?;
                let target = TextTarget { worksheet_part: part.clone(), address };
                if !mapped_targets.contains(&target) || shared_target_uses.get(&index).is_none_or(|uses| !uses.contains(&target)) {
                    return Err(transaction_error(format!("shared-string item {index} is also used by an unmapped cell")));
                }
                if edits.get(&target) != Some(&CellEdit::Text(new_value.clone())) {
                    return Err(transaction_error(format!("shared-string item {index} has a conflicting cell edit")));
                }
            }
        }
        let shared_xml = package.get(&shared_part).ok_or_else(|| transaction_error("shared strings part is missing"))?;
        let shared_nodes = preserved_xml::inspect(shared_xml)?;
        let item_nodes = child_indices(&shared_nodes, 0, SHEET_NS, "si");
        for (index, value) in shared_edits {
            let item = *item_nodes.get(index).ok_or_else(|| transaction_error("shared-string item index is out of range"))?;
            let text_nodes = descendants(&shared_nodes, item, SHEET_NS, "t");
            if text_nodes.len() != 1 || !child_indices(&shared_nodes, item, SHEET_NS, "r").is_empty() {
                return Err(transaction_error("rich shared strings are read-only"));
            }
            add_text_node_splice(
                shared_xml,
                &shared_nodes,
                *text_nodes.first().ok_or_else(|| transaction_error("shared-string text is missing"))?,
                &value,
                splices.entry(shared_part.clone()).or_default(),
            )?;
        }
    }

    let mut replacements = BTreeMap::new();
    for (part, part_splices) in splices {
        let original = package.get(&part).ok_or_else(|| transaction_error("updated XLSX part disappeared"))?;
        replacements.insert(part, apply_splices(original, part_splices)?);
    }
    let entries =
        package.entries().map(|(name, bytes)| (name.to_string(), replacements.remove(name).unwrap_or_else(|| bytes.to_vec()))).collect::<Vec<_>>();
    if !replacements.is_empty() {
        return Err(transaction_error("XLSX update referenced an unplanned part"));
    }
    let output = zip_entries(&entries)?;
    let reopened = Package::open(&output).map_err(|error| transaction_error(format!("updated XLSX package is invalid: {error}")))?;
    for (part, _) in edits
        .keys()
        .map(|target| target.worksheet_part.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|part| reopened.get(part).map(|bytes| (part, bytes)))
    {
        let _ = preserved_xml::inspect(reopened.get(part).ok_or_else(|| transaction_error("updated worksheet is missing"))?)?;
    }
    Ok(output)
}

fn find_cell(nodes: &[Element], address: &str) -> Option<usize> {
    descendants(nodes, 0, SHEET_NS, "c")
        .into_iter()
        .find(|index| nodes.get(*index).and_then(|node| node.attrs.get(&qualified("", "r"))).is_some_and(|value| value == address))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

    fn workbook(with_formula: bool, shared: bool, bad_numeric_type: bool) -> Vec<u8> {
        let (a1, b1, b2, b3) = if shared {
            (
                r#"<c r="A1" t="s"><v>0</v></c>"#,
                r#"<c r="B1" t="s"><v>1</v></c>"#,
                r#"<c r="B2" t="s"><v>2</v></c>"#,
                r#"<c r="B3" t="s"><v>1</v></c>"#,
            )
        } else {
            (
                r#"<c r="A1" t="inlineStr"><is><t>Series one</t></is></c>"#,
                r#"<c r="B1" t="inlineStr"><is><t>Same</t></is></c>"#,
                r#"<c r="B2" t="inlineStr"><is><t>Other</t></is></c>"#,
                r#"<c r="B3" t="inlineStr"><is><t>Same</t></is></c>"#,
            )
        };
        let c2 = if with_formula { r#"<c r="C2"><f>40+2</f><v>42</v></c>"# } else { r#"<c r="C2"><v>42</v></c>"# };
        let c1 = if bad_numeric_type { r#"<c r="C1" t="inlineStr"><is><t>ten</t></is></c>"# } else { r#"<c r="C1"><v>10</v></c>"# };
        let mut files = BTreeMap::from([
            ("[Content_Types].xml".to_string(), br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_vec()),
            ("_rels/.rels".to_string(), format!(r#"<Relationships xmlns="{REL_NS}"><Relationship Id="office" Type="{XLSX_OFFICE}officeDocument" Target="xl/workbook.xml"/></Relationships>"#).into_bytes()),
            ("xl/workbook.xml".to_string(), br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="sheet1"/></sheets></workbook>"#.to_vec()),
            ("xl/_rels/workbook.xml.rels".to_string(), format!(r#"<Relationships xmlns="{REL_NS}"><Relationship Id="sheet1" Type="{XLSX_OFFICE}worksheet" Target="worksheets/sheet1.xml"/>{}</Relationships>"#, if shared { format!(r#"<Relationship Id="strings" Type="{XLSX_OFFICE}sharedStrings" Target="sharedStrings.xml"/>"#) } else { String::new() }).into_bytes()),
            ("xl/worksheets/sheet1.xml".to_string(), format!(r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1">{a1}{b1}{c1}<c r="D1" t="inlineStr"><is><t>Series two</t></is></c></row><row r="2">{b2}{c2}<c r="D2"><v>7</v></c></row><row r="3">{b3}<c r="C3"><v>84</v></c><c r="D3"><v>9</v></c></row><row r="4"><c r="D4"><v>12</v></c></row></sheetData></worksheet>"#).into_bytes()),
        ]);
        if shared {
            files.insert("xl/sharedStrings.xml".into(), br#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>Series one</t></si><si><t>Same</t></si><si><t>Other</t></si></sst>"#.to_vec());
        }
        zip(&files)
    }

    fn chart(series_two: bool) -> Vec<u8> {
        chart_direction(series_two, "col")
    }

    fn chart_direction(series_two: bool, direction: &str) -> Vec<u8> {
        let second = if series_two {
            r#"<c:ser><c:tx><c:strRef><c:f>Data!D1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>Series two</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:f>Data!B1:B3</c:f><c:strCache><c:ptCount val="3"/><c:pt idx="0"><c:v>Same</c:v></c:pt><c:pt idx="1"><c:v>Other</c:v></c:pt><c:pt idx="2"><c:v>Same</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:f>Data!D2:D4</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="3"/><c:pt idx="0"><c:v>7</c:v></c:pt><c:pt idx="1"><c:v>9</c:v></c:pt><c:pt idx="2"><c:v>12</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser>"#
        } else {
            ""
        };
        format!(r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:f>Data!A1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>Series one</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:f>Data!B1:B3</c:f><c:strCache><c:ptCount val="3"/><c:pt idx="0"><c:v>Same</c:v></c:pt><c:pt idx="1"><c:v>Other</c:v></c:pt><c:pt idx="2"><c:v>Same</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:f>Data!C1:C3</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="3"/><c:pt idx="0"><c:v>10</c:v></c:pt><c:pt idx="1"><c:v>42</c:v></c:pt><c:pt idx="2"><c:v>84</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser>{second}<c:axId val="1"/></c:barChart></c:plotArea><c:legend><c:legendPos val="r"/></c:legend></c:chart><c:externalData r:id="book"/></c:chartSpace>"#)
            .replace("<c:barDir val=\"col\"/>", &format!("<c:barDir val=\"{direction}\"/>"))
            .replace("xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"", "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"").into_bytes()
    }

    #[test]
    fn error_bar_inspection_reports_renderable_and_unverified_sources() {
        let xml = br#"<c:ser xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:errBars><c:errDir val="y"/><c:errBarType val="both"/><c:errValType val="fixedVal"/><c:val val="1.5"/></c:errBars><c:errBars><c:errDir val="y"/><c:errBarType val="both"/><c:errValType val="cust"/><c:plus><c:numRef><c:f>Data!B1:B3</c:f><c:numCache><c:ptCount val="3"/><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>1</c:v></c:pt><c:pt idx="2"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:plus><c:minus><c:numRef><c:f>Data!C1:C3</c:f><c:numCache><c:ptCount val="3"/><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>1</c:v></c:pt><c:pt idx="2"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:minus></c:errBars></c:ser>"#;
        let series = xml::parse(xml).unwrap();
        let snapshots = inspect_error_bars(&series, "line", 3);
        assert_eq!(snapshots.len(), 2);
        assert!(snapshots.iter().all(|bar| !bar.renderable && bar.diagnostic.as_deref().is_some_and(|reason| reason.contains("ambiguous"))));

        let formula_only = br#"<c:ser xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:errBars><c:errDir val="y"/><c:errBarType val="both"/><c:errValType val="cust"/><c:plus><c:numRef><c:f>Data!B1:B3</c:f></c:numRef></c:plus><c:minus><c:numLit><c:ptCount val="3"/><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>1</c:v></c:pt><c:pt idx="2"><c:v>1</c:v></c:pt></c:numLit></c:minus></c:errBars></c:ser>"#;
        let series = xml::parse(formula_only).unwrap();
        let snapshot = inspect_error_bars(&series, "scatter", 3);
        assert_eq!(snapshot.len(), 1);
        assert!(!snapshot[0].renderable);
        assert!(snapshot[0].diagnostic.as_deref().is_some_and(|reason| reason.contains("embedded workbook")));
    }

    fn scatter_chart(series_two: bool) -> Vec<u8> {
        let second = if series_two {
            r#"<c:ser><c:idx val="1"/><c:order val="1"/><c:tx><c:strRef><c:f>Data!D1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>Series two</c:v></c:pt></c:strCache></c:strRef></c:tx><c:xVal><c:numRef><c:f>Data!E1:E3</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="3"/><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2</c:v></c:pt><c:pt idx="2"><c:v>3</c:v></c:pt></c:numCache></c:numRef></c:xVal><c:yVal><c:numRef><c:f>Data!F1:F3</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="3"/><c:pt idx="0"><c:v>30</c:v></c:pt><c:pt idx="1"><c:v>40</c:v></c:pt><c:pt idx="2"><c:v>50</c:v></c:pt></c:numCache></c:numRef></c:yVal></c:ser>"#
        } else {
            ""
        };
        format!(r#"<c:chartSpace xmlns:c="{CHART_NS}" xmlns:r="{DOC_REL_NS}"><c:chart><c:plotArea><c:scatterChart><c:scatterStyle val="marker"/><c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:strRef><c:f>Data!A1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>Series one</c:v></c:pt></c:strCache></c:strRef></c:tx><c:xVal><c:numRef><c:f>Data!C1:C3</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="3"/><c:pt idx="0"><c:v>10</c:v></c:pt><c:pt idx="1"><c:v>42</c:v></c:pt><c:pt idx="2"><c:v>84</c:v></c:pt></c:numCache></c:numRef></c:xVal><c:yVal><c:numRef><c:f>Data!D2:D4</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="3"/><c:pt idx="0"><c:v>7</c:v></c:pt><c:pt idx="1"><c:v>9</c:v></c:pt><c:pt idx="2"><c:v>12</c:v></c:pt></c:numCache></c:numRef></c:yVal></c:ser>{second}<c:axId val="1"/><c:axId val="2"/></c:scatterChart><c:valAx><c:axId val="1"/><c:axPos val="b"/></c:valAx><c:valAx><c:axId val="2"/><c:axPos val="l"/></c:valAx></c:plotArea><c:legend><c:legendPos val="r"/></c:legend></c:chart><c:externalData r:id="book"/></c:chartSpace>"#)
            .into_bytes()
    }

    fn scatter_workbook() -> Vec<u8> {
        let mut parts = Package::open(&workbook(false, false, false))
            .unwrap()
            .entries()
            .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
            .collect::<BTreeMap<_, _>>();
        let sheet = String::from_utf8(parts["xl/worksheets/sheet1.xml"].clone()).unwrap();
        let sheet = sheet
            .replace(r#"<row r="1">"#, r#"<row r="1"><c r="E1"><v>1</v></c><c r="F1"><v>30</v></c>"#)
            .replace(r#"<row r="2">"#, r#"<row r="2"><c r="E2"><v>2</v></c><c r="F2"><v>40</v></c>"#)
            .replace(r#"<row r="3">"#, r#"<row r="3"><c r="E3"><v>3</v></c><c r="F3"><v>50</v></c>"#);
        parts.insert("xl/worksheets/sheet1.xml".into(), sheet.into_bytes());
        zip(&parts)
    }

    fn rels(external: bool) -> Vec<Rel> {
        vec![Rel { id: "book".into(), kind: format!("{XLSX_OFFICE}package"), target: "word/embeddings/book.xlsx".into(), external }]
    }

    fn zip(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, bytes) in files {
            writer.start_file(path, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn docx_fixture(shared: bool, second_series: bool, formula: bool, external: bool) -> Vec<u8> {
        let base = crate::write(&wordcraft_doc::Document::from_text("transaction fixture")).unwrap();
        let mut files = Package::open(&base).unwrap().entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        let chart_xml = chart(second_series);
        let embedded = workbook(formula, shared, false);
        let document_rels = files.get("word/_rels/document.xml.rels").unwrap().clone();
        let document_rels = String::from_utf8(document_rels).unwrap().replace(
            "</Relationships>",
            &format!(r#"<Relationship Id="chartForTransaction" Type="{XLSX_OFFICE}chart" Target="charts/chart1.xml"/></Relationships>"#),
        );
        files.insert("word/_rels/document.xml.rels".into(), document_rels.into_bytes());
        let content_types = files.get("[Content_Types].xml").unwrap().clone();
        let content_types = String::from_utf8(content_types).unwrap().replace(
            "</Types>",
            r#"<Override PartName="/word/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/><Override PartName="/word/embeddings/book.xlsx" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"/></Types>"#,
        );
        files.insert("[Content_Types].xml".into(), content_types.into_bytes());
        files.insert("word/charts/chart1.xml".into(), chart_xml);
        files.insert(
            "word/charts/_rels/chart1.xml.rels".into(),
            format!(r#"<Relationships xmlns="{REL_NS}"><Relationship Id="book" Type="{XLSX_OFFICE}package" Target="../embeddings/book.xlsx"{}/></Relationships>"#, if external { r#" TargetMode="External""# } else { "" }).into_bytes(),
        );
        files.insert("word/embeddings/book.xlsx".into(), embedded);
        zip(&files)
    }

    fn axis_docx_fixture() -> Vec<u8> {
        let base = docx_fixture(false, false, false, false);
        let package = Package::open(&base).unwrap();
        let mut files = package.entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        let chart = String::from_utf8(files.get("word/charts/chart1.xml").unwrap().clone()).unwrap()
            .replace("<c:axId val=\"1\"/></c:barChart>", "<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>")
            .replace(
                "</c:barChart></c:plotArea>",
                "</c:barChart><c:catAx><c:axId val=\"1\"/><c:axPos val=\"b\"/><c:crossAx val=\"2\"/></c:catAx><c:valAx><c:axId val=\"2\"/><c:axPos val=\"l\"/><c:crossAx val=\"1\"/></c:valAx></c:plotArea>",
            );
        files.insert("word/charts/chart1.xml".into(), chart.into_bytes());
        zip(&files)
    }

    fn scatter_docx_fixture(second_series: bool) -> Vec<u8> {
        let base = docx_fixture(false, false, false, false);
        let mut files = Package::open(&base).unwrap().entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        files.insert("word/charts/chart1.xml".into(), scatter_chart(second_series));
        files.insert("word/embeddings/book.xlsx".into(), scatter_workbook());
        zip(&files)
    }

    fn series_format_fixture(kind: &str, marker_only_scatter: bool, vary_pie_colors: bool) -> Vec<u8> {
        let base = if kind == "scatter" { scatter_docx_fixture(false) } else { axis_docx_fixture() };
        let package = Package::open(&base).unwrap();
        let mut files = package.entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        let mut chart = String::from_utf8(files.get("word/charts/chart1.xml").unwrap().clone()).unwrap();
        if !chart.contains("xmlns:a=") {
            chart = chart.replace(
                "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"",
                "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"",
            );
        }
        if kind != "column" && kind != "bar" && kind != "scatter" {
            let local = match kind {
                "line" => "lineChart",
                "area" => "areaChart",
                "pie" => "pieChart",
                "doughnut" => "doughnutChart",
                _ => panic!("unknown test chart family"),
            };
            chart = chart.replace("<c:barChart>", &format!("<c:{local}>"));
            chart = chart.replace("</c:barChart>", &format!("</c:{local}>"));
            chart = chart.replace("<c:barDir val=\"col\"/>", "");
            if matches!(kind, "pie" | "doughnut") {
                chart =
                    chart.replace(&format!("<c:{local}>"), &format!("<c:{local}><c:varyColors val=\"{}\"/>", if vary_pie_colors { 1 } else { 0 }));
            }
        } else if kind == "bar" {
            chart = chart.replace("<c:barDir val=\"col\"/>", "<c:barDir val=\"bar\"/>");
        }
        let format = if marker_only_scatter {
            "<c:marker><c:spPr><a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill></c:spPr></c:marker>"
        } else if matches!(kind, "line" | "scatter") {
            "<c:spPr><a:ln w=\"28575\"><a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill></a:ln></c:spPr>"
        } else {
            "<c:spPr><a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill></c:spPr>"
        };
        chart = chart.replacen("<c:tx>", &format!("{format}<c:tx>"), 1);
        files.insert("word/charts/chart1.xml".into(), chart.into_bytes());
        zip(&files)
    }

    fn mapped(result: Classification) -> ChartMapping {
        match result {
            Classification::Mappable(v) => *v,
            other => panic!("expected mapping, got {other:?}"),
        }
    }

    #[test]
    fn numeric_axis_bounds_and_safe_number_formats_are_transactional() {
        let source = axis_docx_fixture();
        let original = Package::open(&source).unwrap();
        let chart_part = "word/charts/chart1.xml";
        let snapshot = inspect_chart_data(&source, chart_part).unwrap();
        assert_eq!(snapshot.horizontal_axis.as_ref().map(|axis| axis.role.as_str()), Some("category"));
        assert!(!snapshot.horizontal_axis.as_ref().is_some_and(|axis| axis.editable));
        let vertical = snapshot.vertical_axis.as_ref().unwrap();
        assert_eq!(vertical.role, "value");
        assert!(vertical.editable);
        assert_eq!(vertical.minimum, None);

        let plan = prepare_chart_edit(&source, chart_part).unwrap();
        let output = apply_chart_edit(
            &source,
            &plan,
            &ChartDataChanges {
                series: vec![SeriesChanges::default()],
                vertical_axis: Some(AxisChanges {
                    minimum: Some(AxisBoundChange::Manual { value: -2.5 }),
                    maximum: Some(AxisBoundChange::Manual { value: 100.0 }),
                    number_format: Some("0.00".into()),
                }),
                ..Default::default()
            },
        )
        .unwrap();
        let updated = inspect_chart_data(&output, chart_part).unwrap();
        let vertical = updated.vertical_axis.as_ref().unwrap();
        assert_eq!(vertical.minimum, Some(-2.5));
        assert_eq!(vertical.maximum, Some(100.0));
        assert_eq!(vertical.number_format.as_deref(), Some("0.00"));
        let updated_package = Package::open(&output).unwrap();
        assert_eq!(updated_package.get("word/embeddings/book.xlsx"), original.get("word/embeddings/book.xlsx"));
        let chart_xml = std::str::from_utf8(updated_package.get(chart_part).unwrap()).unwrap();
        assert!(chart_xml.contains("<c:max val=\"100\"/>"));
        assert!(chart_xml.contains("<c:min val=\"-2.5\"/>"));
        assert!(chart_xml.contains("<c:numFmt formatCode=\"0.00\" sourceLinked=\"0\"/>"));

        let auto_plan = prepare_chart_edit(&output, chart_part).unwrap();
        let automatic = apply_chart_edit(
            &output,
            &auto_plan,
            &ChartDataChanges {
                series: vec![SeriesChanges::default()],
                vertical_axis: Some(AxisChanges { minimum: Some(AxisBoundChange::Auto), maximum: Some(AxisBoundChange::Auto), number_format: None }),
                ..Default::default()
            },
        )
        .unwrap();
        let automatic_snapshot = inspect_chart_data(&automatic, chart_part).unwrap();
        assert_eq!(automatic_snapshot.vertical_axis.as_ref().unwrap().minimum, None);
        assert_eq!(automatic_snapshot.vertical_axis.as_ref().unwrap().maximum, None);
        assert_eq!(Package::open(&automatic).unwrap().get("word/embeddings/book.xlsx"), original.get("word/embeddings/book.xlsx"));
    }

    #[test]
    fn series_colors_and_line_widths_are_transactional_without_workbook_changes() {
        let source = axis_docx_fixture();
        let package = Package::open(&source).unwrap();
        let mut files = package.entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        let original_chart = String::from_utf8(files.get("word/charts/chart1.xml").unwrap().clone()).unwrap();
        let chart = original_chart
            .replace(
                "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"",
                "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"",
            )
            .replace(
                "<c:ser><c:tx>",
                "<c:ser><c:spPr><a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill></c:spPr><c:tx>",
            );
        files.insert("word/charts/chart1.xml".into(), chart.into_bytes());
        let source = zip(&files);
        let before = Package::open(&source).unwrap();
        let snapshot = inspect_chart_data(&source, "word/charts/chart1.xml").unwrap();
        assert_eq!(snapshot.series[0].color.as_deref(), Some("336699"));
        assert!(snapshot.series[0].color_editable);
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        let output = apply_chart_edit(
            &source,
            &plan,
            &ChartDataChanges { series: vec![SeriesChanges { color: Some("C04050".into()), ..Default::default() }], ..Default::default() },
        )
        .unwrap();
        let after = Package::open(&output).unwrap();
        assert_eq!(inspect_chart_data(&output, "word/charts/chart1.xml").unwrap().series[0].color.as_deref(), Some("C04050"));
        assert_eq!(before.get("word/embeddings/book.xlsx"), after.get("word/embeddings/book.xlsx"));
        for (path, bytes) in before.entries().filter(|(path, _)| *path != "word/charts/chart1.xml") {
            assert_eq!(Some(bytes), after.get(path), "unrelated package part changed: {path}");
        }

        let source = String::from_utf8(before.get("word/charts/chart1.xml").unwrap().to_vec())
            .unwrap()
            .replace("<c:barChart>", "<c:lineChart>")
            .replace("</c:barChart>", "</c:lineChart>")
            .replace("<c:barDir val=\"col\"/>", "")
            .replace(
                "<c:ser><c:spPr><a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill></c:spPr>",
                "<c:ser><c:spPr><a:ln w=\"28575\"><a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill></a:ln></c:spPr>",
            )
            .replace("</a:solidFill></a:ln>", "</a:solidFill><a:prstDash val=\"dash\"/></a:ln>")
            .replace("<c:cat>", "<c:errBars><c:errDir val=\"y\"/><c:errBarType val=\"both\"/><c:errValType val=\"fixedVal\"/></c:errBars><c:cat>");
        let mut files = before.entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        files.insert("word/charts/chart1.xml".into(), source.into_bytes());
        let source = zip(&files);
        let snapshot = inspect_chart_data(&source, "word/charts/chart1.xml").unwrap();
        assert!(snapshot.series[0].color_editable);
        assert!(snapshot.series[0].line_width_editable);
        assert_eq!(snapshot.series[0].line_width, Some(2.25));
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        let output = apply_chart_edit(
            &source,
            &plan,
            &ChartDataChanges {
                series: vec![SeriesChanges { color: Some("0080FF".into()), line_width: Some(1.5), ..Default::default() }],
                ..Default::default()
            },
        )
        .unwrap();
        let updated = inspect_chart_data(&output, "word/charts/chart1.xml").unwrap();
        assert_eq!(updated.series[0].color.as_deref(), Some("0080FF"));
        assert_eq!(updated.series[0].line_width, Some(1.5));
        let after = Package::open(&output).unwrap();
        assert_eq!(before.get("word/embeddings/book.xlsx"), after.get("word/embeddings/book.xlsx"));
        let changed_chart = std::str::from_utf8(after.get("word/charts/chart1.xml").unwrap()).unwrap();
        assert!(changed_chart.contains("<a:prstDash val=\"dash\"/>") && changed_chart.contains("<c:errBars>"));
    }

    #[test]
    fn safe_series_color_edits_cover_chart_families_and_scatter_markers() {
        for (kind, marker_only) in [
            ("column", false),
            ("bar", false),
            ("line", false),
            ("area", false),
            ("scatter", false),
            ("scatter", true),
            ("pie", false),
            ("doughnut", false),
        ] {
            let source = series_format_fixture(kind, marker_only, false);
            let before = Package::open(&source).unwrap();
            let snapshot = inspect_chart_data(&source, "word/charts/chart1.xml").unwrap();
            assert!(snapshot.series[0].color_editable, "{kind} marker_only={marker_only}: {:?}", snapshot.series[0].formatting_diagnostic);
            assert_eq!(snapshot.series[0].color.as_deref(), Some("336699"));
            let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
            let output = apply_chart_edit(
                &source,
                &plan,
                &ChartDataChanges { series: vec![SeriesChanges { color: Some("A0B1C2".into()), ..Default::default() }], ..Default::default() },
            )
            .unwrap();
            let after = Package::open(&output).unwrap();
            assert_eq!(inspect_chart_data(&output, "word/charts/chart1.xml").unwrap().series[0].color.as_deref(), Some("A0B1C2"));
            assert_eq!(before.get("word/embeddings/book.xlsx"), after.get("word/embeddings/book.xlsx"));
        }

        for kind in ["pie", "doughnut"] {
            let source = series_format_fixture(kind, false, true);
            let snapshot = inspect_chart_data(&source, "word/charts/chart1.xml").unwrap();
            assert!(!snapshot.series[0].color_editable, "vary-colors {kind} charts need slice-level editing");
        }
    }

    #[test]
    fn scatter_marker_shape_size_and_color_edit_atomically_without_touching_workbook() {
        let source = series_format_fixture("scatter", false, false);
        let package = Package::open(&source).unwrap();
        let mut files = package.entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        let chart = String::from_utf8(files.get("word/charts/chart1.xml").unwrap().clone()).unwrap();
        let chart = chart.replacen(
            "<c:xVal>",
            r#"<c:marker><c:symbol val="circle"/><c:size val="5"/><c:spPr><a:solidFill><a:srgbClr val="112233"/></a:solidFill></c:spPr></c:marker><c:xVal>"#,
            1,
        );
        files.insert("word/charts/chart1.xml".into(), chart.into_bytes());
        let source = zip(&files);
        let before = Package::open(&source).unwrap();
        let snapshot = inspect_chart_data(&source, "word/charts/chart1.xml").unwrap();
        let series = snapshot.series.first().unwrap();
        assert_eq!(series.marker_symbol.as_deref(), Some("circle"));
        assert!(series.marker_symbol_editable);
        assert_eq!(series.marker_size, Some(5));
        assert!(series.marker_size_editable);
        assert_eq!(series.marker_color.as_deref(), Some("112233"));
        assert!(series.marker_color_editable);

        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        let output = apply_chart_edit(
            &source,
            &plan,
            &ChartDataChanges {
                series: vec![SeriesChanges {
                    marker_symbol: Some("diamond".into()),
                    marker_size: Some(11),
                    marker_color: Some("AABBCC".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .unwrap();
        let after = Package::open(&output).unwrap();
        let updated = inspect_chart_data(&output, "word/charts/chart1.xml").unwrap();
        assert_eq!(updated.series[0].marker_symbol.as_deref(), Some("diamond"));
        assert_eq!(updated.series[0].marker_size, Some(11));
        assert_eq!(updated.series[0].marker_color.as_deref(), Some("AABBCC"));
        assert_eq!(before.get("word/embeddings/book.xlsx"), after.get("word/embeddings/book.xlsx"));
        for (name, bytes) in before.entries().filter(|(name, _)| *name != "word/charts/chart1.xml" && *name != "word/embeddings/book.xlsx") {
            assert_eq!(Some(bytes), after.get(name), "unrelated package part {name}");
        }
        let reopened_plan = prepare_chart_edit(&output, "word/charts/chart1.xml").unwrap();
        assert_eq!(reopened_plan.mapping.series[0].formatting.marker_symbol.as_deref(), Some("diamond"));
    }

    #[test]
    fn unsupported_scatter_marker_markup_and_size_are_rejected_without_package_changes() {
        for (symbol, size) in [("pentagon", "5"), ("circle", "100")] {
            let base = series_format_fixture("scatter", false, false);
            let package = Package::open(&base).unwrap();
            let mut files = package.entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
            let chart = String::from_utf8(files.get("word/charts/chart1.xml").unwrap().clone()).unwrap();
            let chart = chart.replacen("<c:xVal>", &format!(r#"<c:marker><c:symbol val="{symbol}"/><c:size val="{size}"/></c:marker><c:xVal>"#), 1);
            files.insert("word/charts/chart1.xml".into(), chart.into_bytes());
            let source = zip(&files);
            let snapshot = inspect_chart_data(&source, "word/charts/chart1.xml").unwrap();
            assert!(!snapshot.series[0].marker_symbol_editable || !snapshot.series[0].marker_size_editable);
            let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
            assert!(
                apply_chart_edit(
                    &source,
                    &plan,
                    &ChartDataChanges {
                        series: vec![SeriesChanges { marker_symbol: Some("triangle".into()), marker_size: Some(9), ..Default::default() }],
                        ..Default::default()
                    }
                )
                .is_err()
            );
        }
    }

    #[test]
    fn unsupported_series_colors_and_invalid_widths_are_read_only() {
        let source = axis_docx_fixture();
        let package = Package::open(&source).unwrap();
        let mut files = package.entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        let chart = String::from_utf8(files.get("word/charts/chart1.xml").unwrap().clone())
            .unwrap()
            .replace(
                "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"",
                "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"",
            )
            .replace(
                "<c:ser><c:tx>",
                "<c:ser><c:spPr><a:gradFill><a:gsLst><a:gs pos=\"0\"><a:srgbClr val=\"336699\"/></a:gs></a:gsLst></a:gradFill></c:spPr><c:tx>",
            );
        files.insert("word/charts/chart1.xml".into(), chart.into_bytes());
        let source = zip(&files);
        let snapshot = inspect_chart_data(&source, "word/charts/chart1.xml").unwrap();
        assert!(!snapshot.series[0].color_editable);
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        for change in [
            SeriesChanges { color: Some("12345Z".into()), ..Default::default() },
            SeriesChanges { color: Some("FFFFFF".into()), ..Default::default() },
            SeriesChanges { line_width: Some(f64::NAN), ..Default::default() },
        ] {
            let result = apply_chart_edit(&source, &plan, &ChartDataChanges { series: vec![change], ..Default::default() });
            assert!(result.is_err());
        }
    }

    #[test]
    fn numeric_axis_with_unknown_ooxml_child_is_read_only() {
        let source = axis_docx_fixture();
        let package = Package::open(&source).unwrap();
        let mut files = package.entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        let chart = String::from_utf8(files.get("word/charts/chart1.xml").unwrap().clone())
            .unwrap()
            .replace("<c:valAx><c:axId val=\"2\"/>", "<c:valAx><c:axId val=\"2\"/><c:unknownAxisFeature val=\"keep\"/>");
        files.insert("word/charts/chart1.xml".into(), chart.into_bytes());
        let source = zip(&files);
        let snapshot = inspect_chart_data(&source, "word/charts/chart1.xml").unwrap();
        let axis = snapshot.vertical_axis.as_ref().unwrap();
        assert!(!axis.editable);
        assert_eq!(axis.diagnostic.as_deref(), Some("This axis contains unsupported OOXML and remains read-only."));
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        let result = apply_chart_edit(
            &source,
            &plan,
            &ChartDataChanges {
                series: vec![SeriesChanges::default()],
                vertical_axis: Some(AxisChanges { minimum: Some(AxisBoundChange::Manual { value: 0.0 }), maximum: None, number_format: None }),
                ..Default::default()
            },
        );
        assert!(result.is_err());
        assert!(std::str::from_utf8(Package::open(&source).unwrap().get("word/charts/chart1.xml").unwrap()).unwrap().contains("unknownAxisFeature"));
    }

    #[test]
    fn physical_axis_controls_follow_horizontal_bar_orientation_and_scatter_xy_ids() {
        let base = axis_docx_fixture();
        let package = Package::open(&base).unwrap();
        let mut files = package.entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        let chart = String::from_utf8(files.get("word/charts/chart1.xml").unwrap().clone())
            .unwrap()
            .replace("<c:barDir val=\"col\"/>", "<c:barDir val=\"bar\"/>")
            .replace("<c:catAx><c:axId val=\"1\"/><c:axPos val=\"b\"/>", "<c:catAx><c:axId val=\"1\"/><c:axPos val=\"l\"/>")
            .replace("<c:valAx><c:axId val=\"2\"/><c:axPos val=\"l\"/>", "<c:valAx><c:axId val=\"2\"/><c:axPos val=\"b\"/>");
        files.insert("word/charts/chart1.xml".into(), chart.into_bytes());
        let horizontal_bar = zip(&files);
        let snapshot = inspect_chart_data(&horizontal_bar, "word/charts/chart1.xml").unwrap();
        assert_eq!(snapshot.horizontal_axis.as_ref().map(|axis| axis.role.as_str()), Some("value"));
        assert_eq!(snapshot.vertical_axis.as_ref().map(|axis| axis.role.as_str()), Some("category"));
        let plan = prepare_chart_edit(&horizontal_bar, "word/charts/chart1.xml").unwrap();
        let edited_bar = apply_chart_edit(
            &horizontal_bar,
            &plan,
            &ChartDataChanges {
                series: vec![SeriesChanges::default()],
                horizontal_axis: Some(AxisChanges {
                    minimum: Some(AxisBoundChange::Manual { value: 0.0 }),
                    maximum: Some(AxisBoundChange::Manual { value: 100.0 }),
                    number_format: None,
                }),
                ..Default::default()
            },
        )
        .unwrap();
        let edited_bar_snapshot = inspect_chart_data(&edited_bar, "word/charts/chart1.xml").unwrap();
        assert_eq!(edited_bar_snapshot.horizontal_axis.as_ref().unwrap().minimum, Some(0.0));
        assert_eq!(edited_bar_snapshot.vertical_axis.as_ref().unwrap().role, "category");

        let scatter = scatter_docx_fixture(false);
        let scatter_before = Package::open(&scatter).unwrap();
        let scatter_snapshot = inspect_chart_data(&scatter, "word/charts/chart1.xml").unwrap();
        assert_eq!(scatter_snapshot.horizontal_axis.as_ref().map(|axis| axis.role.as_str()), Some("scatterX"));
        assert_eq!(scatter_snapshot.vertical_axis.as_ref().map(|axis| axis.role.as_str()), Some("scatterY"));
        let scatter_plan = prepare_chart_edit(&scatter, "word/charts/chart1.xml").unwrap();
        let scaled_scatter = apply_chart_edit(
            &scatter,
            &scatter_plan,
            &ChartDataChanges {
                series: vec![SeriesChanges::default()],
                horizontal_axis: Some(AxisChanges {
                    minimum: Some(AxisBoundChange::Manual { value: 0.0 }),
                    maximum: Some(AxisBoundChange::Manual { value: 100.0 }),
                    number_format: None,
                }),
                vertical_axis: Some(AxisChanges { minimum: None, maximum: None, number_format: Some("0.00E+00".into()) }),
                ..Default::default()
            },
        )
        .unwrap();
        let scaled_snapshot = inspect_chart_data(&scaled_scatter, "word/charts/chart1.xml").unwrap();
        assert_eq!(scaled_snapshot.horizontal_axis.as_ref().unwrap().role, "scatterX");
        assert_eq!(scaled_snapshot.horizontal_axis.as_ref().unwrap().minimum, Some(0.0));
        assert_eq!(scaled_snapshot.vertical_axis.as_ref().unwrap().role, "scatterY");
        assert_eq!(scaled_snapshot.vertical_axis.as_ref().unwrap().number_format.as_deref(), Some("0.00E+00"));
        assert_eq!(Package::open(&scaled_scatter).unwrap().get("word/embeddings/book.xlsx"), scatter_before.get("word/embeddings/book.xlsx"));
    }

    #[test]
    fn axis_transactions_reject_invalid_bounds_logarithmic_and_reversed_axes() {
        let source = axis_docx_fixture();
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        let invalid = ChartDataChanges {
            series: vec![SeriesChanges::default()],
            vertical_axis: Some(AxisChanges {
                minimum: Some(AxisBoundChange::Manual { value: 20.0 }),
                maximum: Some(AxisBoundChange::Manual { value: 10.0 }),
                number_format: None,
            }),
            ..Default::default()
        };
        assert!(apply_chart_edit(&source, &plan, &invalid).is_err());
        let invalid_number = ChartDataChanges {
            series: vec![SeriesChanges::default()],
            vertical_axis: Some(AxisChanges { minimum: Some(AxisBoundChange::Manual { value: f64::NAN }), maximum: None, number_format: None }),
            ..Default::default()
        };
        assert!(apply_chart_edit(&source, &plan, &invalid_number).is_err());

        let original = Package::open(&source).unwrap();
        for scaling in ["<c:logBase val=\"10\"/>", "<c:orientation val=\"maxMin\"/>"] {
            let mut files = original.entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
            let chart = String::from_utf8(files.get("word/charts/chart1.xml").unwrap().clone()).unwrap();
            let chart = chart.replace("<c:valAx><c:axId val=\"2\"/>", &format!("<c:valAx><c:axId val=\"2\"/><c:scaling>{scaling}</c:scaling>"));
            files.insert("word/charts/chart1.xml".into(), chart.into_bytes());
            let unsafe_docx = zip(&files);
            let unsafe_snapshot = inspect_chart_data(&unsafe_docx, "word/charts/chart1.xml").unwrap();
            assert!(!unsafe_snapshot.vertical_axis.as_ref().unwrap().editable);
            let unsafe_plan = prepare_chart_edit(&unsafe_docx, "word/charts/chart1.xml").unwrap();
            assert!(
                apply_chart_edit(
                    &unsafe_docx,
                    &unsafe_plan,
                    &ChartDataChanges {
                        series: vec![SeriesChanges::default()],
                        vertical_axis: Some(AxisChanges {
                            minimum: Some(AxisBoundChange::Manual { value: 0.0 }),
                            maximum: None,
                            number_format: None,
                        }),
                        ..Default::default()
                    }
                )
                .is_err()
            );
        }

        let mut files = original.entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        let chart = String::from_utf8(files.get("word/charts/chart1.xml").unwrap().clone()).unwrap().replace(
            "<c:valAx><c:axId val=\"2\"/><c:axPos val=\"l\"/>",
            "<c:valAx><c:axId val=\"2\"/><c:axPos val=\"l\"/><c:numFmt formatCode=\"yyyy-mm-dd\" sourceLinked=\"0\"/>",
        );
        files.insert("word/charts/chart1.xml".into(), chart.into_bytes());
        let custom_format_docx = zip(&files);
        let custom_snapshot = inspect_chart_data(&custom_format_docx, "word/charts/chart1.xml").unwrap();
        assert!(custom_snapshot.vertical_axis.as_ref().unwrap().editable, "bounds remain independently editable");
        assert!(!custom_snapshot.vertical_axis.as_ref().unwrap().number_format_editable);
        let custom_plan = prepare_chart_edit(&custom_format_docx, "word/charts/chart1.xml").unwrap();
        assert!(
            apply_chart_edit(
                &custom_format_docx,
                &custom_plan,
                &ChartDataChanges {
                    series: vec![SeriesChanges::default()],
                    vertical_axis: Some(AxisChanges { minimum: None, maximum: None, number_format: Some("0".into()) }),
                    ..Default::default()
                }
            )
            .is_err()
        );
        let bound_only = apply_chart_edit(
            &custom_format_docx,
            &custom_plan,
            &ChartDataChanges {
                series: vec![SeriesChanges::default()],
                vertical_axis: Some(AxisChanges { minimum: Some(AxisBoundChange::Manual { value: 0.0 }), maximum: None, number_format: None }),
                ..Default::default()
            },
        )
        .unwrap();
        let updated_chart = Package::open(&bound_only).unwrap();
        assert!(std::str::from_utf8(updated_chart.get("word/charts/chart1.xml").unwrap()).unwrap().contains("yyyy-mm-dd"));
    }

    #[test]
    fn maps_column_series_shared_and_inline_strings_and_keeps_input_immutable() {
        for shared in [false, true] {
            let workbook = workbook(false, shared, false);
            let original = workbook.clone();
            let mapping = mapped(map_chart(&chart(false), &rels(false), "word/embeddings/book.xlsx", &workbook));
            assert_eq!(workbook, original);
            assert_eq!(mapping.chart_kind, "column");
            assert_eq!(mapping.series[0].name.address, "A1");
            assert_eq!(mapping.series[0].categories().iter().map(|c| c.value.as_str()).collect::<Vec<_>>(), ["Same", "Other", "Same"]);
            assert_eq!(mapping.series[0].values().iter().map(|v| v.value).collect::<Vec<_>>(), [10.0, 42.0, 84.0]);
        }
    }

    #[test]
    fn maps_single_and_multiple_scatter_series_to_exact_numeric_x_y_cells() {
        let workbook = scatter_workbook();
        let original = workbook.clone();
        for (multiple, expected_series) in [(false, 1), (true, 2)] {
            let mapping = mapped(map_chart(&scatter_chart(multiple), &rels(false), "word/embeddings/book.xlsx", &workbook));
            assert_eq!(mapping.chart_kind, "scatter");
            assert_eq!(mapping.horizontal_axis.as_ref().map(|axis| axis.role.as_str()), Some("scatterX"));
            assert_eq!(mapping.vertical_axis.as_ref().map(|axis| axis.role.as_str()), Some("scatterY"));
            assert!(mapping.horizontal_axis.as_ref().is_some_and(|axis| axis.editable));
            assert!(mapping.vertical_axis.as_ref().is_some_and(|axis| axis.editable));
            assert_eq!(mapping.series.len(), expected_series);
            assert_eq!(mapping.series[0].name.address, "A1");
            assert_eq!(mapping.series[0].x_values().iter().map(|cell| cell.address.as_str()).collect::<Vec<_>>(), ["C1", "C2", "C3"]);
            assert_eq!(mapping.series[0].x_values().iter().map(|cell| cell.value).collect::<Vec<_>>(), [10.0, 42.0, 84.0]);
            assert_eq!(mapping.series[0].values().iter().map(|cell| cell.address.as_str()).collect::<Vec<_>>(), ["D2", "D3", "D4"]);
            assert_eq!(mapping.series[0].values().iter().map(|cell| cell.value).collect::<Vec<_>>(), [7.0, 9.0, 12.0]);
            assert!(mapping.series[0].categories().is_empty());
            if multiple {
                assert_eq!(mapping.series[1].name.address, "D1");
                assert_eq!(mapping.series[1].x_values().iter().map(|cell| cell.address.as_str()).collect::<Vec<_>>(), ["E1", "E2", "E3"]);
                assert_eq!(mapping.series[1].values().iter().map(|cell| cell.address.as_str()).collect::<Vec<_>>(), ["F1", "F2", "F3"]);
            }
        }
        assert_eq!(workbook, original, "mapping is read-only");
    }

    #[test]
    fn rejects_unsafe_or_inconsistent_scatter_datasets() {
        let chart = scatter_chart(false);
        let scatter_book_bytes = scatter_workbook();
        assert!(matches!(
            map_chart(&chart, &rels(true), "word/embeddings/book.xlsx", &scatter_book_bytes),
            Classification::Unsupported(reason) if reason.contains("external")
        ));
        let formula_workbook = workbook(true, false, false);
        assert!(matches!(
            map_chart(&chart, &rels(false), "word/embeddings/book.xlsx", &formula_workbook),
            Classification::Unsupported(reason) if reason.contains("formula-driven")
        ));
        let bad_length = String::from_utf8(chart.clone()).unwrap().replace("Data!D2:D4", "Data!D2:D3");
        assert!(matches!(
            map_chart(bad_length.as_bytes(), &rels(false), "word/embeddings/book.xlsx", &scatter_book_bytes),
            Classification::Ambiguous(reason) if reason.contains("same nonzero")
        ));
        let noncontiguous = String::from_utf8(chart.clone()).unwrap().replace("Data!C1:C3", "Data!C1,C3");
        assert!(matches!(
            map_chart(noncontiguous.as_bytes(), &rels(false), "word/embeddings/book.xlsx", &scatter_book_bytes),
            Classification::Unsupported(reason) if reason.contains("noncontiguous")
        ));
        let mismatched_cache = String::from_utf8(chart.clone()).unwrap().replace("<c:v>42</c:v>", "<c:v>43</c:v>");
        assert!(matches!(
            map_chart(mismatched_cache.as_bytes(), &rels(false), "word/embeddings/book.xlsx", &scatter_book_bytes),
            Classification::Inconsistent(reason) if reason.contains("cache")
        ));
        let overlap = String::from_utf8(chart).unwrap().replace(
            "Data!D2:D4</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val=\"3\"/><c:pt idx=\"0\"><c:v>7</c:v></c:pt><c:pt idx=\"1\"><c:v>9</c:v></c:pt><c:pt idx=\"2\"><c:v>12</c:v>",
            "Data!C1:C3</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val=\"3\"/><c:pt idx=\"0\"><c:v>10</c:v></c:pt><c:pt idx=\"1\"><c:v>42</c:v></c:pt><c:pt idx=\"2\"><c:v>84</c:v>",
        );
        assert!(matches!(
            map_chart(overlap.as_bytes(), &rels(false), "word/embeddings/book.xlsx", &scatter_book_bytes),
            Classification::Ambiguous(reason) if reason.contains("overlapping")
        ));
    }

    #[test]
    fn scatter_transaction_updates_x_y_caches_and_cells_atomically() {
        let source = scatter_docx_fixture(true);
        let original_parts = Package::open(&source).unwrap();
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        let changes = ChartDataChanges {
            legend_position: None,
            chart_title: None,
            horizontal_axis_title: None,
            vertical_axis_title: None,
            horizontal_axis: None,
            vertical_axis: None,
            series: vec![
                SeriesChanges { x_values: Some(vec![11.0, 22.0, 33.0]), values: Some(vec![110.0, 220.0, 330.0]), ..Default::default() },
                SeriesChanges { x_values: Some(vec![4.0, 5.0, 6.0]), values: Some(vec![44.0, 55.0, 66.0]), ..Default::default() },
            ],
        };
        let output = apply_chart_edit(&source, &plan, &changes).unwrap();
        let updated = Package::open(&output).unwrap();
        let snapshot = inspect_chart_data(&output, "word/charts/chart1.xml").unwrap();
        assert_eq!(snapshot.chart_kind, "scatter");
        assert_eq!(snapshot.series[0].x_values, [11.0, 22.0, 33.0]);
        assert_eq!(snapshot.series[0].values, [110.0, 220.0, 330.0]);
        assert_eq!(snapshot.series[1].x_values, [4.0, 5.0, 6.0]);
        assert_eq!(snapshot.series[1].values, [44.0, 55.0, 66.0]);
        let chart = updated.get("word/charts/chart1.xml").unwrap();
        assert!(chart.windows(b"<c:v>22</c:v>".len()).any(|window| window == b"<c:v>22</c:v>"));
        assert!(chart.windows(b"<c:v>220</c:v>".len()).any(|window| window == b"<c:v>220</c:v>"));
        let book = Package::open(updated.get("word/embeddings/book.xlsx").unwrap()).unwrap();
        let sheet = book.get("xl/worksheets/sheet1.xml").unwrap();
        for value in ["11", "22", "33", "110", "220", "330", "4", "5", "6", "44", "55", "66"] {
            assert!(sheet.windows(format!(">{value}</v>").len()).any(|window| window == format!(">{value}</v>").as_bytes()));
        }
        for (name, bytes) in original_parts.entries() {
            if name != "word/charts/chart1.xml" && name != "word/embeddings/book.xlsx" {
                assert_eq!(updated.get(name), Some(bytes), "unrelated DOCX content changed: {name}");
            }
        }
    }

    #[test]
    fn scatter_transaction_rejects_stale_malformed_and_nonfinite_changes_without_mutation() {
        let source = scatter_docx_fixture(false);
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        for change in [
            ChartDataChanges {
                legend_position: None,
                chart_title: None,
                horizontal_axis_title: None,
                vertical_axis_title: None,
                horizontal_axis: None,
                vertical_axis: None,
                series: vec![SeriesChanges { x_values: Some(vec![1.0]), values: Some(vec![1.0, 2.0, 3.0]), ..Default::default() }],
            },
            ChartDataChanges {
                legend_position: None,
                chart_title: None,
                horizontal_axis_title: None,
                vertical_axis_title: None,
                horizontal_axis: None,
                vertical_axis: None,
                series: vec![SeriesChanges { x_values: Some(vec![1.0, f64::INFINITY, 3.0]), ..Default::default() }],
            },
            ChartDataChanges {
                legend_position: None,
                chart_title: None,
                horizontal_axis_title: None,
                vertical_axis_title: None,
                horizontal_axis: None,
                vertical_axis: None,
                series: vec![SeriesChanges { categories: Some(vec!["a".into(), "b".into(), "c".into()]), ..Default::default() }],
            },
        ] {
            assert!(apply_chart_edit(&source, &plan, &change).is_err());
            assert!(prepare_chart_edit(&source, "word/charts/chart1.xml").is_ok());
        }
        let stale_plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        let mut stale_source = source.clone();
        stale_source.push(0);
        assert!(
            apply_chart_edit(
                &stale_source,
                &stale_plan,
                &ChartDataChanges {
                    legend_position: None,
                    chart_title: None,
                    horizontal_axis_title: None,
                    vertical_axis_title: None,
                    horizontal_axis: None,
                    vertical_axis: None,
                    series: vec![SeriesChanges { values: Some(vec![1.0, 2.0, 3.0]), ..Default::default() }]
                }
            )
            .is_err()
        );
        assert_eq!(source, scatter_docx_fixture(false), "failed edits preserve the complete original package bytes");
    }

    #[test]
    fn maps_horizontal_bar_chart_and_keeps_column_orientation_during_transaction() {
        let original_chart = chart_direction(false, "bar");
        let mapping = mapped(map_chart(&original_chart, &rels(false), "word/embeddings/book.xlsx", &workbook(false, false, false)));
        assert_eq!(mapping.chart_kind, "bar");
        assert_eq!(mapping.series[0].categories().iter().map(|cell| cell.value.as_str()).collect::<Vec<_>>(), ["Same", "Other", "Same"]);

        let package = docx_fixture(false, false, false, false);
        let mut parts =
            Package::open(&package).unwrap().entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        parts.insert("word/charts/chart1.xml".into(), original_chart);
        let package = zip(&parts);
        let plan = prepare_chart_edit(&package, "word/charts/chart1.xml").unwrap();
        let updated = apply_chart_edit(
            &package,
            &plan,
            &ChartDataChanges {
                legend_position: None,
                chart_title: None,
                horizontal_axis_title: None,
                vertical_axis_title: None,
                horizontal_axis: None,
                vertical_axis: None,
                series: vec![SeriesChanges {
                    name: Some("Horizontal".into()),
                    categories: Some(vec!["North".into(), "South".into(), "West".into()]),
                    values: Some(vec![11.0, 22.0, 33.0]),
                    ..Default::default()
                }],
            },
        )
        .unwrap();
        let snapshot = inspect_chart_data(&updated, "word/charts/chart1.xml").unwrap();
        assert_eq!(snapshot.chart_kind, "bar");
        assert_eq!(snapshot.series[0].categories, ["North", "South", "West"]);
        assert_eq!(snapshot.series[0].values, [11.0, 22.0, 33.0]);
        let output = Package::open(&updated).unwrap();
        let chart = output.get("word/charts/chart1.xml").unwrap();
        assert!(chart.windows(b"<c:barDir val=\"bar\"/>".len()).any(|window| window == b"<c:barDir val=\"bar\"/>"));
        let workbook = Package::open(output.get("word/embeddings/book.xlsx").unwrap()).unwrap();
        let sheet = workbook.get("xl/worksheets/sheet1.xml").unwrap();
        assert!(sheet.windows(b">11</v>".len()).any(|window| window == b">11</v>"));
    }

    #[test]
    fn maps_and_updates_simple_line_chart_caches_and_workbook_cells() {
        let line_chart = String::from_utf8(chart(false))
            .unwrap()
            .replace("<c:barChart>", "<c:lineChart>")
            .replace("</c:barChart>", "</c:lineChart>")
            .replace("<c:barDir val=\"col\"/>", "")
            .into_bytes();
        let mapping = mapped(map_chart(&line_chart, &rels(false), "word/embeddings/book.xlsx", &workbook(false, false, false)));
        assert_eq!(mapping.chart_kind, "line");

        let base = docx_fixture(false, false, false, false);
        let mut parts = Package::open(&base).unwrap().entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        parts.insert("word/charts/chart1.xml".into(), line_chart);
        let package = zip(&parts);
        let plan = prepare_chart_edit(&package, "word/charts/chart1.xml").unwrap();
        let updated = apply_chart_edit(
            &package,
            &plan,
            &ChartDataChanges {
                legend_position: None,
                chart_title: None,
                horizontal_axis_title: None,
                vertical_axis_title: None,
                horizontal_axis: None,
                vertical_axis: None,
                series: vec![SeriesChanges {
                    name: Some("Line measurements".into()),
                    categories: Some(vec!["First".into(), "Second".into(), "Third".into()]),
                    values: Some(vec![15.0, 25.0, 35.0]),
                    ..Default::default()
                }],
            },
        )
        .unwrap();
        let snapshot = inspect_chart_data(&updated, "word/charts/chart1.xml").unwrap();
        assert_eq!(snapshot.chart_kind, "line");
        assert_eq!(snapshot.series[0].name, "Line measurements");
        assert_eq!(snapshot.series[0].categories, ["First", "Second", "Third"]);
        assert_eq!(snapshot.series[0].values, [15.0, 25.0, 35.0]);
        let output = Package::open(&updated).unwrap();
        let chart = output.get("word/charts/chart1.xml").unwrap();
        assert!(chart.windows(b"<c:lineChart>".len()).any(|window| window == b"<c:lineChart>"));
        let workbook = Package::open(output.get("word/embeddings/book.xlsx").unwrap()).unwrap();
        let sheet = workbook.get("xl/worksheets/sheet1.xml").unwrap();
        assert!(sheet.windows(b">15</v>".len()).any(|window| window == b">15</v>"));
    }

    #[test]
    fn maps_and_updates_simple_area_chart_caches_and_workbook_cells() {
        let area_chart = String::from_utf8(chart(false))
            .unwrap()
            .replace("<c:barChart>", "<c:areaChart>")
            .replace("</c:barChart>", "</c:areaChart>")
            .replace("<c:barDir val=\"col\"/>", "");
        let mapping = mapped(map_chart(area_chart.as_bytes(), &rels(false), "word/embeddings/book.xlsx", &workbook(false, false, false)));
        assert_eq!(mapping.chart_kind, "area");

        let base = docx_fixture(false, false, false, false);
        let mut parts = Package::open(&base).unwrap().entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        parts.insert("word/charts/chart1.xml".into(), area_chart.into_bytes());
        let package = zip(&parts);
        let plan = prepare_chart_edit(&package, "word/charts/chart1.xml").unwrap();
        let updated = apply_chart_edit(
            &package,
            &plan,
            &ChartDataChanges {
                legend_position: None,
                chart_title: None,
                horizontal_axis_title: None,
                vertical_axis_title: None,
                horizontal_axis: None,
                vertical_axis: None,
                series: vec![SeriesChanges {
                    name: Some("Area measurements".into()),
                    categories: Some(vec!["First".into(), "Second".into(), "Third".into()]),
                    values: Some(vec![16.0, 26.0, 36.0]),
                    ..Default::default()
                }],
            },
        )
        .unwrap();
        let snapshot = inspect_chart_data(&updated, "word/charts/chart1.xml").unwrap();
        assert_eq!(snapshot.chart_kind, "area");
        assert_eq!(snapshot.series[0].name, "Area measurements");
        assert_eq!(snapshot.series[0].categories, ["First", "Second", "Third"]);
        assert_eq!(snapshot.series[0].values, [16.0, 26.0, 36.0]);
        let output = Package::open(&updated).unwrap();
        assert!(output.get("word/charts/chart1.xml").unwrap().windows(b"<c:areaChart>".len()).any(|window| window == b"<c:areaChart>"));
        let workbook = Package::open(output.get("word/embeddings/book.xlsx").unwrap()).unwrap();
        let sheet = workbook.get("xl/worksheets/sheet1.xml").unwrap();
        assert!(sheet.windows(b">16</v>".len()).any(|window| window == b">16</v>"));
    }

    #[test]
    fn maps_single_series_pie_and_doughnut_charts_and_rejects_multiple_series() {
        for (local, label) in [("pieChart", "pie"), ("doughnutChart", "doughnut")] {
            let xml = String::from_utf8(chart(false))
                .unwrap()
                .replace("<c:barChart>", &format!("<c:{local}>"))
                .replace("</c:barChart>", &format!("</c:{local}>"))
                .replace("<c:barDir val=\"col\"/>", "");
            let mapping = mapped(map_chart(xml.as_bytes(), &rels(false), "word/embeddings/book.xlsx", &workbook(false, false, false)));
            assert_eq!(mapping.chart_kind, label);

            let base = docx_fixture(false, false, false, false);
            let mut parts =
                Package::open(&base).unwrap().entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
            parts.insert("word/charts/chart1.xml".into(), xml.into_bytes());
            let package = zip(&parts);
            let plan = prepare_chart_edit(&package, "word/charts/chart1.xml").unwrap();
            let updated = apply_chart_edit(
                &package,
                &plan,
                &ChartDataChanges {
                    legend_position: None,
                    chart_title: None,
                    horizontal_axis_title: None,
                    vertical_axis_title: None,
                    horizontal_axis: None,
                    vertical_axis: None,
                    series: vec![SeriesChanges {
                        name: Some("Revised series".into()),
                        categories: Some(vec!["Cyan".into(), "Magenta".into(), "Yellow".into()]),
                        values: Some(vec![17.0, 27.0, 37.0]),
                        ..Default::default()
                    }],
                },
            )
            .unwrap();
            let snapshot = inspect_chart_data(&updated, "word/charts/chart1.xml").unwrap();
            assert_eq!(snapshot.chart_kind, label);
            assert_eq!(snapshot.series[0].categories, ["Cyan", "Magenta", "Yellow"]);
            assert_eq!(snapshot.series[0].values, [17.0, 27.0, 37.0]);
        }

        let multiple_series = String::from_utf8(chart(true))
            .unwrap()
            .replace("<c:barChart>", "<c:pieChart>")
            .replace("</c:barChart>", "</c:pieChart>")
            .replace("<c:barDir val=\"col\"/>", "");
        assert!(matches!(
            map_chart(multiple_series.as_bytes(), &rels(false), "word/embeddings/book.xlsx", &workbook(false, false, false)),
            Classification::Unsupported(reason) if reason.contains("exactly one series")
        ));
    }

    #[test]
    fn legend_position_transaction_updates_all_supported_chart_families_without_touching_xlsx() {
        let column = String::from_utf8(chart(false)).unwrap();
        let horizontal = column.replace("<c:barDir val=\"col\"/>", "<c:barDir val=\"bar\"/>");
        let line = column.replace("<c:barChart>", "<c:lineChart>").replace("</c:barChart>", "</c:lineChart>").replace("<c:barDir val=\"col\"/>", "");
        let area = column.replace("<c:barChart>", "<c:areaChart>").replace("</c:barChart>", "</c:areaChart>").replace("<c:barDir val=\"col\"/>", "");
        let pie = column.replace("<c:barChart>", "<c:pieChart>").replace("</c:barChart>", "</c:pieChart>").replace("<c:barDir val=\"col\"/>", "");
        let doughnut =
            column.replace("<c:barChart>", "<c:doughnutChart>").replace("</c:barChart>", "</c:doughnutChart>").replace("<c:barDir val=\"col\"/>", "");
        let variants = [
            ("column", column.into_bytes()),
            ("bar", horizontal.into_bytes()),
            ("line", line.into_bytes()),
            ("area", area.into_bytes()),
            ("pie", pie.into_bytes()),
            ("doughnut", doughnut.into_bytes()),
            ("scatter", scatter_chart(false)),
        ];
        for (kind, chart_xml) in variants {
            let base = if kind == "scatter" { scatter_docx_fixture(false) } else { docx_fixture(false, false, false, false) };
            let mut parts =
                Package::open(&base).unwrap().entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
            parts.insert("word/charts/chart1.xml".into(), chart_xml);
            let package = zip(&parts);
            let before = Package::open(&package).unwrap();
            let workbook_before = before.get("word/embeddings/book.xlsx").unwrap().to_vec();
            let plan = prepare_chart_edit(&package, "word/charts/chart1.xml").unwrap();
            assert_eq!(plan.mapping.chart_kind, kind);
            let output = apply_chart_edit(
                &package,
                &plan,
                &ChartDataChanges {
                    legend_position: Some("t".into()),
                    chart_title: None,
                    horizontal_axis_title: None,
                    vertical_axis_title: None,
                    horizontal_axis: None,
                    vertical_axis: None,
                    series: vec![SeriesChanges::default()],
                },
            )
            .unwrap();
            let after = Package::open(&output).unwrap();
            assert_eq!(after.get("word/embeddings/book.xlsx"), Some(workbook_before.as_slice()), "format-only edit modified {kind} workbook bytes");
            let snapshot = inspect_chart_data(&output, "word/charts/chart1.xml").unwrap();
            assert_eq!(snapshot.legend_position.as_deref(), Some("t"), "legend was not updated for {kind}");
        }
    }

    #[test]
    fn legend_position_edit_rejects_missing_unknown_and_invalid_positions() {
        let source = docx_fixture(false, false, false, false);
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        for position in ["tr", "invalid"] {
            let changes = ChartDataChanges {
                legend_position: Some(position.into()),
                chart_title: None,
                horizontal_axis_title: None,
                vertical_axis_title: None,
                horizontal_axis: None,
                vertical_axis: None,
                series: vec![SeriesChanges::default()],
            };
            assert!(apply_chart_edit(&source, &plan, &changes).is_err());
        }
        let without_legend = source.to_vec();
        let package = Package::open(&without_legend).unwrap();
        let mut parts = package.entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        let chart = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap().replace("<c:legend><c:legendPos val=\"r\"/></c:legend>", "");
        parts.insert("word/charts/chart1.xml".into(), chart.into_bytes());
        let without_legend = zip(&parts);
        let plan = prepare_chart_edit(&without_legend, "word/charts/chart1.xml").unwrap();
        assert!(
            apply_chart_edit(
                &without_legend,
                &plan,
                &ChartDataChanges {
                    legend_position: Some("l".into()),
                    chart_title: None,
                    horizontal_axis_title: None,
                    vertical_axis_title: None,
                    horizontal_axis: None,
                    vertical_axis: None,
                    series: vec![SeriesChanges::default()]
                }
            )
            .is_err()
        );
    }

    #[test]
    fn simple_per_series_data_labels_edit_without_touching_workbook() {
        let base = docx_fixture(false, false, false, false);
        let mut parts = Package::open(&base).unwrap().entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        let chart = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap().replace(
            "</c:ser>",
            r#"<c:dLbls><c:showVal val="1"/><c:showCatName val="0"/><c:showSerName val="0"/><c:dLblPos val="outEnd"/></c:dLbls></c:ser>"#,
        );
        parts.insert("word/charts/chart1.xml".into(), chart.into_bytes());
        let source = zip(&parts);
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        let label_snapshot = plan.mapping.series[0].formatting.data_labels.as_ref().unwrap();
        assert!(label_snapshot.editable);
        assert_eq!(label_snapshot.show_value, Some(true));
        assert_eq!(label_snapshot.position.as_deref(), Some("outEnd"));
        let workbook_before = Package::open(&source).unwrap().get("word/embeddings/book.xlsx").unwrap().to_vec();
        let output = apply_chart_edit(
            &source,
            &plan,
            &ChartDataChanges {
                series: vec![SeriesChanges {
                    data_labels: Some(DataLabelChanges {
                        show_value: Some(false),
                        show_category: Some(true),
                        show_series_name: Some(true),
                        position: Some("inEnd".into()),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .unwrap();
        let package = Package::open(&output).unwrap();
        assert_eq!(package.get("word/embeddings/book.xlsx"), Some(workbook_before.as_slice()));
        let snapshot = inspect_chart_data(&output, "word/charts/chart1.xml").unwrap();
        let labels = snapshot.series[0].data_labels.as_ref().unwrap();
        assert_eq!(labels.show_value, Some(false));
        assert_eq!(labels.show_category, Some(true));
        assert_eq!(labels.show_series_name, Some(true));
        assert_eq!(labels.position.as_deref(), Some("inEnd"));
    }

    #[test]
    fn custom_point_label_xml_is_preserved_and_read_only() {
        let base = docx_fixture(false, false, false, false);
        let mut parts = Package::open(&base).unwrap().entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        let chart = String::from_utf8(parts["word/charts/chart1.xml"].clone())
            .unwrap()
            .replace("</c:ser>", r#"<c:dLbls><c:showVal val="1"/><c:dLbl><c:idx val="0"/><c:tx><c:v>custom</c:v></c:tx></c:dLbl></c:dLbls></c:ser>"#);
        parts.insert("word/charts/chart1.xml".into(), chart.into_bytes());
        let source = zip(&parts);
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        assert!(!plan.mapping.series[0].formatting.data_labels.as_ref().unwrap().editable);
        let result = apply_chart_edit(
            &source,
            &plan,
            &ChartDataChanges {
                series: vec![SeriesChanges {
                    data_labels: Some(DataLabelChanges { show_value: Some(false), ..Default::default() }),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        assert!(result.is_err());
        assert_eq!(source, zip(&parts));
    }

    #[test]
    fn edits_single_run_rich_chart_and_axis_titles_by_physical_axis_position() {
        let mut xml = String::from_utf8(chart(false)).unwrap();
        let title = r#"<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:pPr/><a:r><a:rPr b="1"/><a:t>Original chart</a:t></a:r><a:endParaRPr/></a:p></c:rich></c:tx><c:overlay val="0"/></c:title>"#;
        xml = xml.replace("<c:chart><c:plotArea>", &format!("<c:chart>{title}<c:plotArea>"));
        xml = xml.replace("<c:axId val=\"1\"/></c:barChart>", "<c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>");
        let category_title = title.replace("Original chart", "Category axis");
        let value_title = title.replace("Original chart", "Value axis");
        let axes = format!(
            r#"<c:catAx><c:axId val="1"/><c:axPos val="b"/>{category_title}</c:catAx><c:valAx><c:axId val="2"/><c:axPos val="l"/>{value_title}</c:valAx>"#
        );
        xml = xml.replace("</c:plotArea>", &format!("{axes}</c:plotArea>"));
        xml = xml.replace(
            "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"",
            "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"",
        );
        let mut parts = Package::open(&docx_fixture(false, false, false, false))
            .unwrap()
            .entries()
            .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
            .collect::<BTreeMap<_, _>>();
        parts.insert("word/charts/chart1.xml".into(), xml.clone().into_bytes());
        let package = zip(&parts);
        let snapshot = inspect_chart_data(&package, "word/charts/chart1.xml").unwrap();
        assert_eq!(snapshot.chart_title.as_deref(), Some("Original chart"));
        assert_eq!(snapshot.horizontal_axis_title.as_deref(), Some("Category axis"));
        assert_eq!(snapshot.vertical_axis_title.as_deref(), Some("Value axis"));
        let plan = prepare_chart_edit(&package, "word/charts/chart1.xml").unwrap();
        let changed = apply_chart_edit(
            &package,
            &plan,
            &ChartDataChanges {
                series: vec![SeriesChanges::default()],
                chart_title: Some("Updated chart".into()),
                horizontal_axis_title: Some("Horizontal categories".into()),
                vertical_axis_title: Some("Vertical values".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let changed_snapshot = inspect_chart_data(&changed, "word/charts/chart1.xml").unwrap();
        assert_eq!(changed_snapshot.chart_title.as_deref(), Some("Updated chart"));
        assert_eq!(changed_snapshot.horizontal_axis_title.as_deref(), Some("Horizontal categories"));
        assert_eq!(changed_snapshot.vertical_axis_title.as_deref(), Some("Vertical values"));
        let before = Package::open(&package).unwrap();
        let after = Package::open(&changed).unwrap();
        assert_eq!(before.get("word/embeddings/book.xlsx"), after.get("word/embeddings/book.xlsx"));
        for (part, bytes) in before.entries() {
            if part != "word/charts/chart1.xml" {
                assert_eq!(after.get(part), Some(bytes), "title-only edit changed unrelated part {part}");
            }
        }
        let chart = std::str::from_utf8(after.get("word/charts/chart1.xml").unwrap()).unwrap();
        assert!(chart.contains("<a:rPr b=\"1\"/>"), "run formatting is retained");
        assert!(chart.contains("<c:overlay val=\"0\"/>"), "title properties are retained");

        let horizontal_bar = xml
            .replace("<c:barDir val=\"col\"/>", "<c:barDir val=\"bar\"/>")
            .replace("<c:axPos val=\"b\"/>", "<c:axPos val=\"axis_bottom\"/>")
            .replace("<c:axPos val=\"l\"/>", "<c:axPos val=\"b\"/>")
            .replace("<c:axPos val=\"axis_bottom\"/>", "<c:axPos val=\"l\"/>");
        let horizontal_mapping =
            mapped(map_chart(horizontal_bar.as_bytes(), &rels(false), "word/embeddings/book.xlsx", &workbook(false, false, false)));
        assert_eq!(horizontal_mapping.chart_kind, "bar");
        assert_eq!(horizontal_mapping.horizontal_axis_title.as_deref(), Some("Value axis"));
        assert_eq!(horizontal_mapping.vertical_axis_title.as_deref(), Some("Category axis"));
    }

    #[test]
    fn rejects_absent_complex_and_non_cartesian_axis_titles_without_mutation() {
        let source = docx_fixture(false, false, false, false);
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        assert!(inspect_chart_data(&source, "word/charts/chart1.xml").unwrap().chart_title.is_none());
        assert!(
            apply_chart_edit(
                &source,
                &plan,
                &ChartDataChanges { series: vec![SeriesChanges::default()], chart_title: Some("Add me".into()), ..Default::default() }
            )
            .is_err()
        );

        let mut files = Package::open(&source).unwrap().entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
        let complex_title =
            r#"<c:title><c:tx><c:rich><a:p><a:r><a:t>First</a:t></a:r><a:r><a:rPr i="1"/><a:t> second</a:t></a:r></a:p></c:rich></c:tx></c:title>"#;
        let complex_xml = String::from_utf8(files["word/charts/chart1.xml"].clone())
            .unwrap()
            .replace("<c:chart><c:plotArea>", &format!("<c:chart>{complex_title}<c:plotArea>"))
            .replace(
                "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"",
                "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"",
            );
        files.insert("word/charts/chart1.xml".into(), complex_xml.into_bytes());
        let complex_package = zip(&files);
        let complex_plan = prepare_chart_edit(&complex_package, "word/charts/chart1.xml").unwrap();
        assert!(inspect_chart_data(&complex_package, "word/charts/chart1.xml").unwrap().chart_title.is_none());
        assert!(
            apply_chart_edit(
                &complex_package,
                &complex_plan,
                &ChartDataChanges { series: vec![SeriesChanges::default()], chart_title: Some("Unsafe rewrite".into()), ..Default::default() }
            )
            .is_err()
        );

        let pie = String::from_utf8(chart(false))
            .unwrap()
            .replace("<c:barChart>", "<c:pieChart>")
            .replace("</c:barChart>", "</c:pieChart>")
            .replace("<c:barDir val=\"col\"/>", "");
        let pie_mapping = mapped(map_chart(pie.as_bytes(), &rels(false), "word/embeddings/book.xlsx", &workbook(false, false, false)));
        assert!(pie_mapping.horizontal_axis_title.is_none() && pie_mapping.vertical_axis_title.is_none());
    }

    #[test]
    fn chart_title_edits_work_for_each_supported_chart_family_without_workbook_changes() {
        let column = String::from_utf8(chart(false)).unwrap();
        let horizontal = column.replace("<c:barDir val=\"col\"/>", "<c:barDir val=\"bar\"/>");
        let line = column.replace("<c:barChart>", "<c:lineChart>").replace("</c:barChart>", "</c:lineChart>").replace("<c:barDir val=\"col\"/>", "");
        let area = column.replace("<c:barChart>", "<c:areaChart>").replace("</c:barChart>", "</c:areaChart>").replace("<c:barDir val=\"col\"/>", "");
        let pie = column.replace("<c:barChart>", "<c:pieChart>").replace("</c:barChart>", "</c:pieChart>").replace("<c:barDir val=\"col\"/>", "");
        let doughnut =
            column.replace("<c:barChart>", "<c:doughnutChart>").replace("</c:barChart>", "</c:doughnutChart>").replace("<c:barDir val=\"col\"/>", "");
        let title = r#"<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr i="1"/><a:t>Old</a:t></a:r></a:p></c:rich></c:tx></c:title>"#;
        let variants = [
            ("column", column.into_bytes(), false),
            ("bar", horizontal.into_bytes(), false),
            ("line", line.into_bytes(), false),
            ("area", area.into_bytes(), false),
            ("pie", pie.into_bytes(), false),
            ("doughnut", doughnut.into_bytes(), false),
            ("scatter", scatter_chart(false), true),
        ];
        for (kind, chart_xml, scatter) in variants {
            let base = if scatter { scatter_docx_fixture(false) } else { docx_fixture(false, false, false, false) };
            let mut parts =
                Package::open(&base).unwrap().entries().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect::<BTreeMap<_, _>>();
            let source_chart = String::from_utf8(chart_xml).unwrap().replace("<c:chart><c:plotArea>", &format!("<c:chart>{title}<c:plotArea>"));
            let source_chart = source_chart.replace(
                "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"",
                "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"",
            );
            parts.insert("word/charts/chart1.xml".into(), source_chart.into_bytes());
            let package = zip(&parts);
            let xlsx_before = Package::open(&package).unwrap().get("word/embeddings/book.xlsx").unwrap().to_vec();
            let plan = prepare_chart_edit(&package, "word/charts/chart1.xml").unwrap();
            assert_eq!(plan.mapping.chart_kind, kind);
            let updated = apply_chart_edit(
                &package,
                &plan,
                &ChartDataChanges { series: vec![SeriesChanges::default()], chart_title: Some("Updated & reviewed".into()), ..Default::default() },
            )
            .unwrap();
            let snapshot = inspect_chart_data(&updated, "word/charts/chart1.xml").unwrap();
            assert_eq!(snapshot.chart_title.as_deref(), Some("Updated & reviewed"), "chart-title mapping failed for {kind}");
            let after = Package::open(&updated).unwrap();
            assert_eq!(after.get("word/embeddings/book.xlsx"), Some(xlsx_before.as_slice()), "title edit modified {kind} workbook");
        }
    }

    #[test]
    fn maps_multiple_series_to_exact_cells() {
        let m = mapped(map_chart(&chart(true), &rels(false), "word/embeddings/book.xlsx", &workbook(false, false, false)));
        assert_eq!(m.series.len(), 2);
        assert_eq!(m.series[1].name.address, "D1");
        assert_eq!(m.series[1].values()[2].address, "D4");
        assert_eq!(m.series[1].categories(), m.series[0].categories());
    }

    #[test]
    fn rejects_formula_cells_external_links_missing_sheets_and_bad_ranges() {
        assert!(
            matches!(map_chart(&chart(false), &rels(false), "word/embeddings/book.xlsx", &workbook(true, false, false)), Classification::Unsupported(r) if r.contains("formula-driven"))
        );
        assert!(
            matches!(map_chart(&chart(false), &rels(true), "word/embeddings/book.xlsx", &workbook(false, false, false)), Classification::Unsupported(r) if r.contains("external"))
        );
        let missing = chart(false).iter().map(|b| *b as char).collect::<String>().replace("Data!", "Absent!");
        assert!(
            matches!(map_chart(missing.as_bytes(), &rels(false), "word/embeddings/book.xlsx", &workbook(false, false, false)), Classification::Unsupported(r) if r.contains("missing worksheet"))
        );
        let noncontiguous = chart(false).iter().map(|b| *b as char).collect::<String>().replace("Data!C1:C3", "Data!C1,C3");
        assert!(
            matches!(map_chart(noncontiguous.as_bytes(), &rels(false), "word/embeddings/book.xlsx", &workbook(false, false, false)), Classification::Unsupported(r) if r.contains("noncontiguous"))
        );
        let malformed = chart(false).iter().map(|b| *b as char).collect::<String>().replace("Data!C1:C3", "Data!XFE1:XFE3");
        assert!(matches!(
            map_chart(malformed.as_bytes(), &rels(false), "word/embeddings/book.xlsx", &workbook(false, false, false)),
            Classification::Unsupported(_)
        ));
        assert!(
            matches!(map_chart(&chart(false), &[], "word/embeddings/book.xlsx", &workbook(false, false, false)), Classification::Unsupported(r) if r.contains("no embedded workbook"))
        );
        assert!(
            matches!(map_chart(&chart(false), &rels(false), "word/embeddings/other.xlsx", &workbook(false, false, false)), Classification::Inconsistent(r) if r.contains("does not match"))
        );
    }

    #[test]
    fn classifies_cache_mismatches_and_missing_workbook_sheets() {
        let mismatch = chart(false).iter().map(|b| *b as char).collect::<String>().replace("<c:v>84</c:v>", "<c:v>85</c:v>");
        assert!(
            matches!(map_chart(mismatch.as_bytes(), &rels(false), "word/embeddings/book.xlsx", &workbook(false, false, false)), Classification::Inconsistent(r) if r.contains("cache"))
        );
        let mut files = BTreeMap::new();
        files.insert("[Content_Types].xml".into(), br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/missing.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_vec());
        files.insert("xl/workbook.xml".into(), br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="sheet1"/></sheets></workbook>"#.to_vec());
        files.insert("xl/_rels/workbook.xml.rels".into(), format!(r#"<Relationships xmlns="{REL_NS}"><Relationship Id="sheet1" Type="{XLSX_OFFICE}worksheet" Target="worksheets/missing.xml"/></Relationships>"#).into_bytes());
        let xlsx = zip(&files);
        assert!(
            matches!(map_chart(&chart(false), &rels(false), "word/embeddings/book.xlsx", &xlsx), Classification::Unsupported(r) if r.contains("worksheet part is missing"))
        );
        let bad_type = workbook(false, false, true);
        assert!(
            matches!(map_chart(&chart(false), &rels(false), "word/embeddings/book.xlsx", &bad_type), Classification::Inconsistent(r) if r.contains("text cell"))
        );
        let bad_length = chart(false).iter().map(|b| *b as char).collect::<String>().replace("<c:ptCount val=\"3\"/>", "<c:ptCount val=\"2\"/>");
        assert!(
            matches!(map_chart(bad_length.as_bytes(), &rels(false), "word/embeddings/book.xlsx", &workbook(false, false, false)), Classification::Inconsistent(r) if r.contains("cache"))
        );
    }

    #[test]
    fn range_parser_accepts_absolute_and_quoted_sheet_references() {
        let range = parse_range("'Research data'!$B$2:$B$4").unwrap();
        assert_eq!(range.sheet, "Research data");
        assert_eq!(range.cells.iter().map(cell_name).collect::<Vec<_>>(), ["B2", "B3", "B4"]);
        assert!(matches!(parse_range("Data!A1:A2,C1:C2"), Err(Reason::Unsupported(_))));
    }

    #[test]
    fn chart_transaction_updates_inline_cells_and_caches_and_preserves_other_parts() {
        let source = docx_fixture(false, false, false, false);
        let source_before = source.clone();
        let original = Package::open(&source).unwrap();
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        let changes = ChartDataChanges {
            legend_position: None,
            chart_title: None,
            horizontal_axis_title: None,
            vertical_axis_title: None,
            horizontal_axis: None,
            vertical_axis: None,
            series: vec![SeriesChanges {
                name: Some("Updated series".into()),
                categories: Some(vec!["Alpha".into(), "Beta & gamma".into(), "Gamma".into()]),
                values: Some(vec![12.5, 45.0, 91.25]),
                ..Default::default()
            }],
        };
        let output = apply_chart_edit(&source, &plan, &changes).unwrap();
        assert_eq!(source, source_before, "the source byte buffer is immutable");
        let updated = Package::open(&output).unwrap();
        let rels = updated.rels_checked("word/charts/chart1.xml").unwrap();
        let workbook = updated.get("word/embeddings/book.xlsx").unwrap();
        let map = mapped(map_chart(updated.get("word/charts/chart1.xml").unwrap(), &rels.list, "word/embeddings/book.xlsx", workbook));
        assert_eq!(map.series[0].name.value, "Updated series");
        assert_eq!(map.series[0].categories().iter().map(|c| c.value.as_str()).collect::<Vec<_>>(), ["Alpha", "Beta & gamma", "Gamma"]);
        assert_eq!(map.series[0].values().iter().map(|cell| cell.value).collect::<Vec<_>>(), [12.5, 45.0, 91.25]);
        for (name, bytes) in original.entries() {
            if name != "word/charts/chart1.xml" && name != "word/embeddings/book.xlsx" {
                assert_eq!(updated.get(name), Some(bytes), "unrelated DOCX part changed: {name}");
            }
        }
        let old_xlsx = Package::open(original.get("word/embeddings/book.xlsx").unwrap()).unwrap();
        let new_xlsx = Package::open(workbook).unwrap();
        for (name, bytes) in old_xlsx.entries() {
            if name != "xl/worksheets/sheet1.xml" {
                assert_eq!(new_xlsx.get(name), Some(bytes), "unrelated XLSX part changed: {name}");
            }
        }
    }

    #[test]
    fn chart_transaction_updates_shared_strings_and_multiple_series() {
        let source = docx_fixture(true, true, false, false);
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        let output = apply_chart_edit(
            &source,
            &plan,
            &ChartDataChanges {
                legend_position: None,
                chart_title: None,
                horizontal_axis_title: None,
                vertical_axis_title: None,
                horizontal_axis: None,
                vertical_axis: None,
                series: vec![
                    SeriesChanges {
                        name: Some("Primary".into()),
                        categories: Some(vec!["Repeated".into(), "New category".into(), "Repeated".into()]),
                        values: Some(vec![11.0, 22.0, 33.0]),
                        ..Default::default()
                    },
                    SeriesChanges {
                        name: Some("Secondary".into()),
                        categories: Some(vec!["Repeated".into(), "New category".into(), "Repeated".into()]),
                        values: Some(vec![44.0, 55.0, 66.0]),
                        ..Default::default()
                    },
                ],
            },
        )
        .unwrap();
        let package = Package::open(&output).unwrap();
        let chart_rels = package.rels_checked("word/charts/chart1.xml").unwrap();
        let mapping = mapped(map_chart(
            package.get("word/charts/chart1.xml").unwrap(),
            &chart_rels.list,
            "word/embeddings/book.xlsx",
            package.get("word/embeddings/book.xlsx").unwrap(),
        ));
        assert_eq!(mapping.series[0].name.value, "Primary");
        assert_eq!(mapping.series[1].name.value, "Secondary");
        assert_eq!(mapping.series[0].categories()[1].value, "New category");
        assert_eq!(mapping.series[0].values()[2].value, 33.0);
        assert_eq!(mapping.series[1].values()[1].value, 55.0);
        let xlsx = Package::open(package.get("word/embeddings/book.xlsx").unwrap()).unwrap();
        assert!(std::str::from_utf8(xlsx.get("xl/sharedStrings.xml").unwrap()).unwrap().contains("Primary"));
    }

    #[test]
    fn chart_transaction_rejects_unsafe_stale_or_conflicting_edits_without_output() {
        let source = docx_fixture(false, true, false, false);
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        let conflicting = ChartDataChanges {
            legend_position: None,
            chart_title: None,
            horizontal_axis_title: None,
            vertical_axis_title: None,
            horizontal_axis: None,
            vertical_axis: None,
            series: vec![
                SeriesChanges { categories: Some(vec!["First".into(), "Second".into(), "Third".into()]), ..Default::default() },
                SeriesChanges { categories: Some(vec!["Conflict".into(), "Second".into(), "Third".into()]), ..Default::default() },
            ],
        };
        assert!(apply_chart_edit(&source, &plan, &conflicting).is_err());
        let invalid_number = ChartDataChanges {
            legend_position: None,
            chart_title: None,
            horizontal_axis_title: None,
            vertical_axis_title: None,
            horizontal_axis: None,
            vertical_axis: None,
            series: vec![SeriesChanges { values: Some(vec![f64::NAN, 1.0, 2.0]), ..Default::default() }, SeriesChanges::default()],
        };
        assert!(apply_chart_edit(&source, &plan, &invalid_number).is_err());
        let mut stale = source.clone();
        let last = stale.len().saturating_sub(1);
        if let Some(byte) = stale.get_mut(last) {
            *byte ^= 1;
        }
        assert!(
            apply_chart_edit(
                &stale,
                &plan,
                &ChartDataChanges {
                    legend_position: None,
                    chart_title: None,
                    horizontal_axis_title: None,
                    vertical_axis_title: None,
                    horizontal_axis: None,
                    vertical_axis: None,
                    series: vec![SeriesChanges { values: Some(vec![1.0, 2.0, 3.0]), ..Default::default() }, SeriesChanges::default()]
                }
            )
            .is_err()
        );
        assert!(prepare_chart_edit(&source, "word/charts/../document.xml").is_err());
        assert!(prepare_chart_edit(b"not a zip", "word/charts/chart1.xml").is_err());
    }

    #[test]
    fn chart_transaction_refuses_formula_driven_and_external_workbooks() {
        let formula = docx_fixture(false, false, true, false);
        assert!(prepare_chart_edit(&formula, "word/charts/chart1.xml").is_err());
        let external = docx_fixture(false, false, false, true);
        assert!(prepare_chart_edit(&external, "word/charts/chart1.xml").is_err());
    }

    #[test]
    fn chart_transaction_rejects_cache_mismatch_rich_strings_and_unmapped_shared_aliases() {
        let mut mismatched = Package::open(&docx_fixture(false, false, false, false))
            .unwrap()
            .entries()
            .map(|(n, b)| (n.to_string(), b.to_vec()))
            .collect::<BTreeMap<_, _>>();
        let bad_chart =
            String::from_utf8(mismatched.get("word/charts/chart1.xml").unwrap().clone()).unwrap().replace("<c:v>84</c:v>", "<c:v>85</c:v>");
        mismatched.insert("word/charts/chart1.xml".into(), bad_chart.into_bytes());
        assert!(prepare_chart_edit(&zip(&mismatched), "word/charts/chart1.xml").is_err());

        let source = docx_fixture(true, false, false, false);
        let source_before = source.clone();
        let rich_changes = ChartDataChanges {
            legend_position: None,
            chart_title: None,
            horizontal_axis_title: None,
            vertical_axis_title: None,
            horizontal_axis: None,
            vertical_axis: None,
            series: vec![SeriesChanges { categories: Some(vec!["Changed".into(), "Other".into(), "Changed".into()]), ..Default::default() }],
        };
        let mut rich_files = Package::open(&source).unwrap().entries().map(|(n, b)| (n.to_string(), b.to_vec())).collect::<BTreeMap<_, _>>();
        let mut xlsx = Package::open(rich_files.get("word/embeddings/book.xlsx").unwrap())
            .unwrap()
            .entries()
            .map(|(n, b)| (n.to_string(), b.to_vec()))
            .collect::<BTreeMap<_, _>>();
        let strings = String::from_utf8(xlsx.get("xl/sharedStrings.xml").unwrap().clone())
            .unwrap()
            .replace("<si><t>Other</t></si>", "<si><r><t>Oth</t></r><r><t>er</t></r></si>");
        xlsx.insert("xl/sharedStrings.xml".into(), strings.into_bytes());
        rich_files.insert("word/embeddings/book.xlsx".into(), zip(&xlsx));
        let rich_source = zip(&rich_files);
        let rich_plan = prepare_chart_edit(&rich_source, "word/charts/chart1.xml").unwrap();
        assert!(apply_chart_edit(&rich_source, &rich_plan, &rich_changes).is_err());

        let mut alias_files = Package::open(&source).unwrap().entries().map(|(n, b)| (n.to_string(), b.to_vec())).collect::<BTreeMap<_, _>>();
        let mut xlsx = Package::open(alias_files.get("word/embeddings/book.xlsx").unwrap())
            .unwrap()
            .entries()
            .map(|(n, b)| (n.to_string(), b.to_vec()))
            .collect::<BTreeMap<_, _>>();
        let sheet = String::from_utf8(xlsx.get("xl/worksheets/sheet1.xml").unwrap().to_vec()).unwrap().replacen(
            "</row>",
            "<c r=\"E1\" t=\"s\"><v>0</v></c></row>",
            1,
        );
        xlsx.insert("xl/worksheets/sheet1.xml".into(), sheet.into_bytes());
        alias_files.insert("word/embeddings/book.xlsx".into(), zip(&xlsx));
        let alias_source = zip(&alias_files);
        let alias_plan = prepare_chart_edit(&alias_source, "word/charts/chart1.xml").unwrap();
        let label_edit = ChartDataChanges {
            legend_position: None,
            chart_title: None,
            horizontal_axis_title: None,
            vertical_axis_title: None,
            horizontal_axis: None,
            vertical_axis: None,
            series: vec![SeriesChanges { name: Some("Renamed".into()), ..Default::default() }],
        };
        assert!(apply_chart_edit(&alias_source, &alias_plan, &label_edit).is_err());
        assert_eq!(source, source_before);
    }
}
