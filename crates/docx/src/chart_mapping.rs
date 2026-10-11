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

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Classification {
    Mappable(ChartMapping),
    Unsupported(String),
    Ambiguous(String),
    Inconsistent(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ChartMapping {
    pub chart_kind: String,
    pub workbook_part: String,
    pub series: Vec<SeriesMapping>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SeriesMapping {
    pub name: MappedCell<String>,
    pub categories: Vec<MappedCell<String>>,
    pub values: Vec<MappedCell<f64>>,
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
        Ok(mapping) => Classification::Mappable(mapping),
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
    if kinds.len() != 1 || kinds[0].name != "c:barChart" {
        return Err(unsupported("only a single bar/column chart is supported"));
    }
    let kind = kinds[0];
    if kind.child("c:barDir").and_then(|e| e.attr("val")) != Some("col") || contains_local(kind, "extLst", 0) {
        return Err(unsupported("only plain column charts without extensions are supported"));
    }
    let series: Vec<_> = kind.children("c:ser").collect();
    if series.is_empty() || series.len() > MAX_SERIES {
        return Err(unsupported("chart has no series or exceeds the series limit"));
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
        let category_source = ser.child("c:cat").ok_or_else(|| unsupported("category labels have no source"))?;
        let value_source = ser.child("c:val").ok_or_else(|| unsupported("series values have no source"))?;
        if name_source.child("c:strRef").is_none() || category_source.child("c:strRef").is_none() || value_source.child("c:numRef").is_none() {
            return Err(unsupported("series name/category/value reference types are unsupported"));
        }
        let name_ref = ref_formula(name_source, "series name")?;
        let cat_ref = ref_formula(category_source, "category labels")?;
        let val_ref = ref_formula(value_source, "series values")?;
        if name_ref.cells.len() != 1 || cat_ref.cells.len() != val_ref.cells.len() || cat_ref.cells.is_empty() {
            return Err(ambiguous("series ranges have incompatible dimensions"));
        }
        mapped_cells = mapped_cells
            .checked_add(name_ref.cells.len())
            .and_then(|count| count.checked_add(cat_ref.cells.len()))
            .and_then(|count| count.checked_add(val_ref.cells.len()))
            .ok_or_else(|| unsupported("mapped cell budget overflow"))?;
        if mapped_cells > MAX_MAPPED_CELLS {
            return Err(unsupported("chart exceeds the mapped cell budget"));
        }
        if name_ref.sheet != cat_ref.sheet || cat_ref.sheet != val_ref.sheet {
            return Err(unsupported("series ranges span different worksheets"));
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
        let categories = read_text_range(&cat_ref, &cat_ref.sheet, path, cells)?;
        let values = read_number_range(&val_ref, &val_ref.sheet, path, cells)?;
        verify_string_cache(ser.child("c:tx"), std::slice::from_ref(&name))?;
        verify_string_cache(ser.child("c:cat"), &categories)?;
        verify_number_cache(ser.child("c:val"), &values)?;
        mapped.push(SeriesMapping { name, categories, values });
    }
    Ok(ChartMapping { chart_kind: "column".into(), workbook_part: workbook_part.into(), series: mapped })
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
pub(crate) struct ChartEditPlan {
    chart_part: String,
    workbook_part: String,
    package_digest: [u8; 32],
    mapping: ChartMapping,
}

#[derive(Clone, Debug, Default)]
#[allow(dead_code)] // Internal transaction API is intentionally staged before editor integration.
pub(crate) struct ChartDataChanges {
    pub series: Vec<SeriesChanges>,
}

#[derive(Clone, Debug, Default)]
#[allow(dead_code)] // Internal transaction API is intentionally staged before editor integration.
pub(crate) struct SeriesChanges {
    pub name: Option<String>,
    pub categories: Option<Vec<String>>,
    pub values: Option<Vec<f64>>,
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
#[allow(dead_code)] // Internal transaction API is intentionally staged before editor integration.
pub(crate) fn prepare_chart_edit(docx: &[u8], chart_part: &str) -> Result<ChartEditPlan, DocxError> {
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
        Classification::Mappable(mapping) => mapping,
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

/// Apply a constrained chart edit as one all-or-nothing package transaction.
///
/// No caller-owned bytes are mutated. Both chart caches and exactly mapped workbook cells are
/// staged, revalidated, and returned as a new DOCX only after package and mapping validation pass.
#[allow(dead_code)] // Internal transaction API is intentionally staged before editor integration.
pub(crate) fn apply_chart_edit(docx: &[u8], plan: &ChartEditPlan, changes: &ChartDataChanges) -> Result<Vec<u8>, DocxError> {
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
        Classification::Mappable(mapping) => mapping,
        Classification::Unsupported(reason) => return Err(transaction_error(format!("chart became read-only: {reason}"))),
        Classification::Ambiguous(reason) => return Err(transaction_error(format!("chart mapping became ambiguous: {reason}"))),
        Classification::Inconsistent(reason) => return Err(transaction_error(format!("chart data became inconsistent: {reason}"))),
    };
    if fresh != plan.mapping {
        return Err(transaction_error("stale chart mapping: mapped cells or source values changed"));
    }

    let mut expected = fresh.clone();
    let chart_bytes = update_chart_caches(chart, changes)?;
    let cell_edits = build_cell_edits(&fresh, changes, &mut expected)?;
    let workbook_bytes = apply_workbook_edits(workbook, &cell_edits)?;
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
        Classification::Mappable(mapping) if mapping == expected => Ok(output),
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
    for (source, change) in mapping.series.iter().zip(&changes.series) {
        if let Some(name) = &change.name {
            any = true;
            validate_label(name)?;
            if !matches!(&source.name.source, CellSource::SharedString(_) | CellSource::InlineString | CellSource::StringValue) {
                return Err(transaction_error("series name is not stored in an editable string cell"));
            }
        }
        if let Some(categories) = &change.categories {
            any = true;
            if categories.len() != source.categories.len() {
                return Err(transaction_error("category edit length must match the validated chart range"));
            }
            for label in categories {
                validate_label(label)?;
            }
            if source
                .categories
                .iter()
                .any(|cell| !matches!(&cell.source, CellSource::SharedString(_) | CellSource::InlineString | CellSource::StringValue))
            {
                return Err(transaction_error("category range contains a non-string cell"));
            }
        }
        if let Some(values) = &change.values {
            any = true;
            if values.len() != source.values.len() {
                return Err(transaction_error("numeric edit length must match the validated chart range"));
            }
            if values.iter().any(|value| !value.is_finite()) {
                return Err(transaction_error("chart values must be finite numbers"));
            }
        }
    }
    if !any {
        return Err(transaction_error("change set contains no edits"));
    }
    Ok(())
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
        if let Some(values) = &change.categories {
            for ((cell, value), expected_cell) in series.categories.iter().zip(values).zip(&mut expected_series.categories) {
                add_text_edit(&mut edits, cell, value)?;
                expected_cell.value = value.clone();
            }
        }
        if let Some(values) = &change.values {
            for ((cell, value), expected_cell) in series.values.iter().zip(values).zip(&mut expected_series.values) {
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

fn update_chart_caches(chart: &[u8], changes: &ChartDataChanges) -> Result<Vec<u8>, DocxError> {
    let nodes = preserved_xml::inspect(chart)?;
    let root = nodes.first().ok_or_else(|| transaction_error("chart XML has no root"))?;
    let bar_charts = descendants(&nodes, 0, CHART_NS, "barChart");
    if bar_charts.len() != 1 {
        return Err(transaction_error("chart no longer has one supported column-chart cache"));
    }
    let chart_series = child_indices(&nodes, *bar_charts.first().ok_or_else(|| transaction_error("chart plot area is missing"))?, CHART_NS, "ser");
    if chart_series.len() != changes.series.len() {
        return Err(transaction_error("chart series changed after mapping"));
    }
    let _ = root;
    let mut splices = Vec::new();
    for (series_index, (series_node, change)) in chart_series.iter().zip(&changes.series).enumerate() {
        for (container_name, cache_name, values) in [
            ("tx", "strCache", change.name.as_ref().map(|value| vec![value.clone()])),
            ("cat", "strCache", change.categories.clone()),
            ("val", "numCache", change.values.as_ref().map(|values| values.iter().map(ToString::to_string).collect())),
        ] {
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
        let second = if series_two {
            r#"<c:ser><c:tx><c:strRef><c:f>Data!D1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>Series two</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:f>Data!B1:B3</c:f><c:strCache><c:ptCount val="3"/><c:pt idx="0"><c:v>Same</c:v></c:pt><c:pt idx="1"><c:v>Other</c:v></c:pt><c:pt idx="2"><c:v>Same</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:f>Data!D2:D4</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="3"/><c:pt idx="0"><c:v>7</c:v></c:pt><c:pt idx="1"><c:v>9</c:v></c:pt><c:pt idx="2"><c:v>12</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser>"#
        } else {
            ""
        };
        format!(r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:f>Data!A1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>Series one</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:f>Data!B1:B3</c:f><c:strCache><c:ptCount val="3"/><c:pt idx="0"><c:v>Same</c:v></c:pt><c:pt idx="1"><c:v>Other</c:v></c:pt><c:pt idx="2"><c:v>Same</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:f>Data!C1:C3</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="3"/><c:pt idx="0"><c:v>10</c:v></c:pt><c:pt idx="1"><c:v>42</c:v></c:pt><c:pt idx="2"><c:v>84</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser>{second}<c:axId val="1"/></c:barChart></c:plotArea></c:chart><c:externalData r:id="book"/></c:chartSpace>"#)
            .replace("xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"", "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"").into_bytes()
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

    fn mapped(result: Classification) -> ChartMapping {
        match result {
            Classification::Mappable(v) => v,
            other => panic!("expected mapping, got {other:?}"),
        }
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
            assert_eq!(mapping.series[0].categories.iter().map(|c| c.value.as_str()).collect::<Vec<_>>(), ["Same", "Other", "Same"]);
            assert_eq!(mapping.series[0].values.iter().map(|v| v.value).collect::<Vec<_>>(), [10.0, 42.0, 84.0]);
        }
    }

    #[test]
    fn maps_multiple_series_to_exact_cells() {
        let m = mapped(map_chart(&chart(true), &rels(false), "word/embeddings/book.xlsx", &workbook(false, false, false)));
        assert_eq!(m.series.len(), 2);
        assert_eq!(m.series[1].name.address, "D1");
        assert_eq!(m.series[1].values[2].address, "D4");
        assert_eq!(m.series[1].categories, m.series[0].categories);
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
            series: vec![SeriesChanges {
                name: Some("Updated series".into()),
                categories: Some(vec!["Alpha".into(), "Beta & gamma".into(), "Gamma".into()]),
                values: Some(vec![12.5, 45.0, 91.25]),
            }],
        };
        let output = apply_chart_edit(&source, &plan, &changes).unwrap();
        assert_eq!(source, source_before, "the source byte buffer is immutable");
        let updated = Package::open(&output).unwrap();
        let rels = updated.rels_checked("word/charts/chart1.xml").unwrap();
        let workbook = updated.get("word/embeddings/book.xlsx").unwrap();
        let map = mapped(map_chart(updated.get("word/charts/chart1.xml").unwrap(), &rels.list, "word/embeddings/book.xlsx", workbook));
        assert_eq!(map.series[0].name.value, "Updated series");
        assert_eq!(map.series[0].categories.iter().map(|c| c.value.as_str()).collect::<Vec<_>>(), ["Alpha", "Beta & gamma", "Gamma"]);
        assert_eq!(map.series[0].values.iter().map(|cell| cell.value).collect::<Vec<_>>(), [12.5, 45.0, 91.25]);
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
                series: vec![
                    SeriesChanges {
                        name: Some("Primary".into()),
                        categories: Some(vec!["Repeated".into(), "New category".into(), "Repeated".into()]),
                        values: Some(vec![11.0, 22.0, 33.0]),
                    },
                    SeriesChanges {
                        name: Some("Secondary".into()),
                        categories: Some(vec!["Repeated".into(), "New category".into(), "Repeated".into()]),
                        values: Some(vec![44.0, 55.0, 66.0]),
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
        assert_eq!(mapping.series[0].categories[1].value, "New category");
        assert_eq!(mapping.series[0].values[2].value, 33.0);
        assert_eq!(mapping.series[1].values[1].value, 55.0);
        let xlsx = Package::open(package.get("word/embeddings/book.xlsx").unwrap()).unwrap();
        assert!(std::str::from_utf8(xlsx.get("xl/sharedStrings.xml").unwrap()).unwrap().contains("Primary"));
    }

    #[test]
    fn chart_transaction_rejects_unsafe_stale_or_conflicting_edits_without_output() {
        let source = docx_fixture(false, true, false, false);
        let plan = prepare_chart_edit(&source, "word/charts/chart1.xml").unwrap();
        let conflicting = ChartDataChanges {
            series: vec![
                SeriesChanges { categories: Some(vec!["First".into(), "Second".into(), "Third".into()]), ..Default::default() },
                SeriesChanges { categories: Some(vec!["Conflict".into(), "Second".into(), "Third".into()]), ..Default::default() },
            ],
        };
        assert!(apply_chart_edit(&source, &plan, &conflicting).is_err());
        let invalid_number = ChartDataChanges {
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
        let label_edit = ChartDataChanges { series: vec![SeriesChanges { name: Some("Renamed".into()), ..Default::default() }] };
        assert!(apply_chart_edit(&alias_source, &alias_plan, &label_edit).is_err());
        assert_eq!(source, source_before);
    }
}
