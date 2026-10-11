//! Read-only validation of the small, safe subset of chart-to-XLSX mappings.
//!
//! This deliberately produces no writable handles. It accepts only contiguous A1 ranges on
//! ordinary worksheets and verifies every displayed cache value against the backing cell.
use std::collections::BTreeMap;

use crate::package::{ContentTypes, EMBEDDED_PARTS, EmbeddedManifest, Package, Rel, rel_is};
use crate::xml::{self, El};

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
    Text(String),
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
                    CellValue::Text(shared.get(index).ok_or_else(|| unsupported("shared-string cell index is missing"))?.clone())
                }
                "inlineStr" => CellValue::Text(cell.child("x:is").ok_or_else(|| unsupported("inline string cell has no value"))?.deep_text()),
                "str" => CellValue::Text(cell.child("x:v").map(El::text).unwrap_or_default()),
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
        Some(CellValue::Text(value)) => value.clone(),
        Some(CellValue::Number(_)) => return Err(inconsistent("text chart reference points to a numeric cell")),
        Some(CellValue::Unsupported(reason)) => return Err(unsupported(reason)),
        None => return Err(inconsistent("chart range references a missing worksheet cell")),
    };
    if value.len() > MAX_LABEL_BYTES {
        return Err(unsupported("worksheet label exceeds the text limit"));
    }
    Ok(MappedCell { worksheet: sheet.to_string(), worksheet_part: sheet_path.to_string(), address: key, value })
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
                Some(CellValue::Text(_)) => return Err(inconsistent("numeric chart reference points to a text cell")),
                Some(CellValue::Unsupported(reason)) => return Err(unsupported(reason)),
                None => return Err(inconsistent("chart range references a missing worksheet cell")),
            };
            Ok(MappedCell { worksheet: sheet.to_string(), worksheet_part: sheet_path.to_string(), address: key, value })
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
            files.insert("xl/sharedStrings.xml".into(), br#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>Series one</t></si><si><t>Same</t></si><si><r><t>Oth</t></r><r><t>er</t></r></si></sst>"#.to_vec());
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
}
