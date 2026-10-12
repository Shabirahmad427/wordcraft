//! Charts WordCraft makes ([`ChartSpec`]) as chart parts (ECMA-376 Part 1 §21.2) and back.
//!
//! [`chart_xml`] writes an in-memory chart with literal data. New DOCX charts use
//! [`chart_package_parts`] to write matching worksheet-backed caches and an embedded workbook.
//! [`chart_items`] draws a chart through the same code that draws chart parts read from files.
//! [`spec_of`] turns a chart part back into the model only when writing that model again gives
//! the same markup, so a chart with anything the model leaves out stays as it was read.

use wordcraft_doc::chart::{ChartSeries, ChartSpec, ChartType, LegendPos};
use wordcraft_doc::graphic::GraphicItem;
use wordcraft_doc::props::Rgb;

use crate::package::{self, Rel};
use crate::xml::{self, El, MAX_DEPTH, Node, W};

const NS_C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// Content type of a chart part.
pub(crate) const CHART_CT: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
pub(crate) const WORKBOOK_CT: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
/// `a:graphicData` URI of a chart.
pub(crate) const CHART_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
/// Axis ids (any pair of distinct numbers will do; Word keeps them).
const CAT_AX: &str = "500000001";
const VAL_AX: &str = "500000002";

/// The chart part for `spec`: a complete XML document. `spec` should be sanitized
/// ([`ChartSpec::sanitize`]); a value that isn't finite is written as a gap either way.
pub fn chart_xml(spec: &ChartSpec) -> String {
    chart_xml_mode(spec, None)
}

/// Build a chart part and a self-contained workbook for a newly inserted chart. The output is
/// validated through the same mapper used by Chart Design before it is accepted for writing.
pub(crate) fn chart_package_parts(spec: &ChartSpec, workbook_rel_id: &str) -> Result<(String, Vec<u8>), crate::DocxError> {
    let spec = spec.clone().sanitized();
    validate_editable_spec(&spec)?;
    let chart = chart_xml_mode(&spec, Some(workbook_rel_id));
    let workbook = chart_workbook(&spec)?;
    let rel = Rel {
        id: workbook_rel_id.to_string(),
        kind: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/package".into(),
        target: "word/embeddings/workbook1.xlsx".into(),
        external: false,
    };
    match crate::chart_mapping::map_chart(chart.as_bytes(), &[rel], "word/embeddings/workbook1.xlsx", &workbook) {
        crate::chart_mapping::Classification::Mappable(_) => Ok((chart, workbook)),
        crate::chart_mapping::Classification::Inconsistent(reason)
            if spec.series.iter().any(|series| series.values.iter().any(Option::is_none)) && reason.contains("references a missing") =>
        {
            // Preserve the supported chart model's explicit gaps as blank cells. The mapper
            // intentionally leaves sparse data read-only because its editor accepts only numbers.
            Ok((chart, workbook))
        }
        crate::chart_mapping::Classification::Unsupported(reason)
        | crate::chart_mapping::Classification::Ambiguous(reason)
        | crate::chart_mapping::Classification::Inconsistent(reason) => {
            Err(crate::DocxError::Preservation(format!("new chart data could not be validated safely: {reason}")))
        }
    }
}

fn validate_editable_spec(spec: &ChartSpec) -> Result<(), crate::DocxError> {
    if spec.series.is_empty() || spec.categories.is_empty() {
        return Err(crate::DocxError::Preservation("new editable charts need at least one series and one data point".into()));
    }
    if spec.kind.is_round() && spec.series.len() != 1 {
        return Err(crate::DocxError::Preservation("editable pie and doughnut charts require exactly one series".into()));
    }
    if spec.series.iter().any(|series| series.name.is_empty() || series.values.len() != spec.categories.len()) {
        return Err(crate::DocxError::Preservation("new charts require named series with one value slot for every category".into()));
    }
    if spec.kind == ChartType::Scatter && spec.categories.iter().any(|category| category.parse::<f64>().ok().is_none_or(|value| !value.is_finite())) {
        return Err(crate::DocxError::Preservation("editable scatter charts require finite numeric X values".into()));
    }
    Ok(())
}

fn chart_xml_mode(spec: &ChartSpec, workbook_rel_id: Option<&str>) -> String {
    let mut w = W::new();
    w.open("c:chartSpace", &[("xmlns:c", NS_C), ("xmlns:a", NS_A), ("xmlns:r", NS_R)]);
    w.empty("c:roundedCorners", &[("val", "0")]);
    w.open("c:chart", &[]);
    if let Some(t) = &spec.title {
        w.open("c:title", &[]);
        w.open("c:tx", &[]);
        w.open("c:rich", &[]);
        w.empty("a:bodyPr", &[]);
        w.empty("a:lstStyle", &[]);
        w.open("a:p", &[]);
        w.open("a:r", &[]);
        w.leaf("a:t", &[], t);
        w.close("a:r");
        w.close("a:p");
        w.close("c:rich");
        w.close("c:tx");
        w.empty("c:overlay", &[("val", "0")]);
        w.close("c:title");
    }
    w.empty("c:autoTitleDeleted", &[("val", if spec.title.is_some() { "0" } else { "1" })]);
    w.open("c:plotArea", &[]);
    w.empty("c:layout", &[]);
    plot(&mut w, spec, workbook_rel_id.is_some());
    w.close("c:plotArea");
    if let Some(pos) = spec.legend {
        w.open("c:legend", &[]);
        let p = match pos {
            LegendPos::Right => "r",
            LegendPos::Left => "l",
            LegendPos::Top => "t",
            LegendPos::Bottom => "b",
        };
        w.empty("c:legendPos", &[("val", p)]);
        w.empty("c:overlay", &[("val", "0")]);
        w.close("c:legend");
    }
    w.empty("c:plotVisOnly", &[("val", "1")]);
    w.empty("c:dispBlanksAs", &[("val", "gap")]);
    w.close("c:chart");
    if let Some(id) = workbook_rel_id {
        w.empty("c:externalData", &[("xmlns:r", NS_R), ("r:id", id)]);
    }
    // A white chart area without a border, in the document's own background colour.
    w.open("c:spPr", &[]);
    w.open("a:solidFill", &[]);
    w.empty("a:schemeClr", &[("val", "bg1")]);
    w.close("a:solidFill");
    w.open("a:ln", &[]);
    w.empty("a:noFill", &[]);
    w.close("a:ln");
    w.close("c:spPr");
    w.close("c:chartSpace");
    w.s
}

fn chart_workbook(spec: &ChartSpec) -> Result<Vec<u8>, crate::DocxError> {
    let mut entries = Vec::new();
    let mut types = W::new();
    types.open("Types", &[("xmlns", "http://schemas.openxmlformats.org/package/2006/content-types")]);
    types.empty("Default", &[("Extension", "rels"), ("ContentType", "application/vnd.openxmlformats-package.relationships+xml")]);
    types.empty("Default", &[("Extension", "xml"), ("ContentType", "application/xml")]);
    types.empty(
        "Override",
        &[("PartName", "/xl/workbook.xml"), ("ContentType", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml")],
    );
    types.empty(
        "Override",
        &[("PartName", "/xl/worksheets/sheet1.xml"), ("ContentType", "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml")],
    );
    types.close("Types");
    entries.push(("[Content_Types].xml".to_string(), types.into_bytes()));

    let mut root_rels = W::new();
    root_rels.open("Relationships", &[("xmlns", "http://schemas.openxmlformats.org/package/2006/relationships")]);
    root_rels.empty(
        "Relationship",
        &[
            ("Id", "rId1"),
            ("Type", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument"),
            ("Target", "xl/workbook.xml"),
        ],
    );
    root_rels.close("Relationships");
    entries.push(("_rels/.rels".to_string(), root_rels.into_bytes()));

    let mut workbook = W::new();
    workbook.open("workbook", &[("xmlns", "http://schemas.openxmlformats.org/spreadsheetml/2006/main"), ("xmlns:r", NS_R)]);
    workbook.open("sheets", &[]);
    workbook.empty("sheet", &[("name", "Data"), ("sheetId", "1"), ("r:id", "rId1")]);
    workbook.close("sheets");
    workbook.close("workbook");
    entries.push(("xl/workbook.xml".to_string(), workbook.into_bytes()));

    let mut wb_rels = W::new();
    wb_rels.open("Relationships", &[("xmlns", "http://schemas.openxmlformats.org/package/2006/relationships")]);
    wb_rels.empty(
        "Relationship",
        &[
            ("Id", "rId1"),
            ("Type", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet"),
            ("Target", "worksheets/sheet1.xml"),
        ],
    );
    wb_rels.close("Relationships");
    entries.push(("xl/_rels/workbook.xml.rels".to_string(), wb_rels.into_bytes()));

    let mut sheet = W::new();
    sheet.open("worksheet", &[("xmlns", "http://schemas.openxmlformats.org/spreadsheetml/2006/main")]);
    sheet.open("sheetData", &[]);
    if spec.kind == ChartType::Scatter {
        open_row(&mut sheet, 1);
        for (index, series) in spec.series.iter().enumerate() {
            let base_col = index
                .checked_mul(3)
                .and_then(|n| n.checked_add(1))
                .ok_or_else(|| crate::DocxError::Limit("chart worksheet column overflow".into()))?;
            add_text_cell(&mut sheet, 1, base_col, &series.name);
        }
        sheet.close("row");
        for (index, x) in spec.categories.iter().enumerate() {
            let row = index + 2;
            open_row(&mut sheet, row);
            let x = x.parse::<f64>().map_err(|_| crate::DocxError::Preservation("scatter X value is invalid".into()))?;
            for (series_index, series) in spec.series.iter().enumerate() {
                let base_col = series_index * 3 + 1;
                add_number_cell(&mut sheet, row, base_col + 1, x);
                if let Some(value) = series.values.get(index).and_then(|value| *value) {
                    add_number_cell(&mut sheet, row, base_col + 2, value);
                }
            }
            sheet.close("row");
        }
    } else {
        open_row(&mut sheet, 1);
        add_text_cell(&mut sheet, 1, 1, "");
        for (index, series) in spec.series.iter().enumerate() {
            add_text_cell(&mut sheet, 1, index + 2, &series.name);
        }
        sheet.close("row");
        for (index, category) in spec.categories.iter().enumerate() {
            let row = index + 2;
            open_row(&mut sheet, row);
            add_text_cell(&mut sheet, row, 1, category);
            for (series_index, series) in spec.series.iter().enumerate() {
                if let Some(value) = series.values.get(index).and_then(|value| *value) {
                    add_number_cell(&mut sheet, row, series_index + 2, value);
                }
            }
            sheet.close("row");
        }
    }
    sheet.close("sheetData");
    sheet.close("worksheet");
    entries.push(("xl/worksheets/sheet1.xml".to_string(), sheet.into_bytes()));
    package::zip_entries(&entries)
}

fn add_text_cell(w: &mut W, row: usize, col: usize, value: &str) {
    let address = format!("{}{}", column_name(col), row);
    w.open("c", &[("r", &address), ("t", "inlineStr")]);
    w.open("is", &[]);
    if value.trim() != value {
        w.leaf("t", &[("xml:space", "preserve")], value);
    } else {
        w.leaf("t", &[], value);
    }
    w.close("is");
    w.close("c");
}

fn add_number_cell(w: &mut W, row: usize, col: usize, value: f64) {
    let address = format!("{}{}", column_name(col), row);
    w.open("c", &[("r", &address)]);
    w.leaf("v", &[], &num(value));
    w.close("c");
}

fn open_row(w: &mut W, row: usize) {
    w.open("row", &[("r", &row.to_string())]);
}

fn column_name(mut col: usize) -> String {
    let mut result = Vec::new();
    while col > 0 {
        let remainder = (col - 1) % 26;
        result.push(char::from(b'A' + remainder as u8));
        col = (col - 1) / 26;
    }
    result.reverse();
    result.into_iter().collect()
}

/// What `spec` draws as, `w` × `h` points, in the colours of `theme` (series take the accents in
/// turn). Empty when the chart has no series.
pub fn chart_items(spec: &ChartSpec, theme: &[Rgb], w: f32, h: f32) -> Vec<GraphicItem> {
    match xml::parse(chart_xml(spec).as_bytes()) {
        Ok(space) => crate::read::chart::chart_items(&space, theme, w, h),
        Err(_) => Vec::new(),
    }
}

/// The chart type element and the series in it, then the axes.
fn plot(w: &mut W, spec: &ChartSpec, embedded: bool) {
    let k = spec.kind;
    let el = match k {
        ChartType::Column | ChartType::ColumnStacked | ChartType::Bar | ChartType::BarStacked => "c:barChart",
        ChartType::Line => "c:lineChart",
        ChartType::Area => "c:areaChart",
        ChartType::Pie => "c:pieChart",
        ChartType::Doughnut => "c:doughnutChart",
        ChartType::Scatter => "c:scatterChart",
    };
    let stacked = matches!(k, ChartType::ColumnStacked | ChartType::BarStacked);
    let horizontal = matches!(k, ChartType::Bar | ChartType::BarStacked);
    w.open(el, &[]);
    match k {
        ChartType::Column | ChartType::ColumnStacked | ChartType::Bar | ChartType::BarStacked => {
            w.empty("c:barDir", &[("val", if horizontal { "bar" } else { "col" })]);
            w.empty("c:grouping", &[("val", if stacked { "stacked" } else { "clustered" })]);
        }
        ChartType::Line | ChartType::Area => w.empty("c:grouping", &[("val", "standard")]),
        ChartType::Scatter => w.empty("c:scatterStyle", &[("val", "lineMarker")]),
        ChartType::Pie | ChartType::Doughnut => {}
    }
    w.empty("c:varyColors", &[("val", if k.is_round() { "1" } else { "0" })]);
    // Pie and doughnut charts show the first series; the others are kept for other types.
    for (i, s) in spec.series.iter().enumerate() {
        series(w, spec, i, s, embedded);
    }
    if spec.data_labels {
        w.open("c:dLbls", &[]);
        for (name, on) in [
            ("c:showLegendKey", false),
            ("c:showVal", true),
            ("c:showCatName", false),
            ("c:showSerName", false),
            ("c:showPercent", false),
            ("c:showBubbleSize", false),
        ] {
            w.empty(name, &[("val", if on { "1" } else { "0" })]);
        }
        w.close("c:dLbls");
    }
    match k {
        ChartType::Column | ChartType::ColumnStacked | ChartType::Bar | ChartType::BarStacked => {
            w.empty("c:gapWidth", &[("val", "150")]);
            if stacked {
                w.empty("c:overlap", &[("val", "100")]);
            }
        }
        ChartType::Line => w.empty("c:marker", &[("val", "1")]),
        ChartType::Pie => w.empty("c:firstSliceAng", &[("val", "0")]),
        ChartType::Doughnut => {
            w.empty("c:firstSliceAng", &[("val", "0")]);
            w.empty("c:holeSize", &[("val", "50")]);
        }
        ChartType::Area | ChartType::Scatter => {}
    }
    if !k.is_round() {
        w.empty("c:axId", &[("val", CAT_AX)]);
        w.empty("c:axId", &[("val", VAL_AX)]);
    }
    w.close(el);
    match k {
        ChartType::Pie | ChartType::Doughnut => {}
        ChartType::Scatter => {
            value_axis(w, CAT_AX, VAL_AX, "b", false, "midCat");
            value_axis(w, VAL_AX, CAT_AX, "l", true, "midCat");
        }
        _ => {
            category_axis(w, if horizontal { "l" } else { "b" });
            value_axis(w, VAL_AX, CAT_AX, if horizontal { "b" } else { "l" }, true, "between");
        }
    }
}

/// One `c:ser` (ECMA-376 element order: idx, order, tx, spPr, marker, cat/xVal, val/yVal, smooth).
fn series(w: &mut W, spec: &ChartSpec, i: usize, s: &ChartSeries, embedded: bool) {
    let k = spec.kind;
    let index = i;
    let i = index.to_string();
    w.open("c:ser", &[]);
    w.empty("c:idx", &[("val", &i)]);
    w.empty("c:order", &[("val", &i)]);
    if embedded {
        let name_col = if k == ChartType::Scatter { index.saturating_mul(3).saturating_add(1) } else { index.saturating_add(2) };
        w.open("c:tx", &[]);
        w.open("c:strRef", &[]);
        let name_col = column_name(name_col);
        w.leaf("c:f", &[], &format!("Data!${name_col}$1"));
        str_cache(w, std::slice::from_ref(&s.name));
        w.close("c:strRef");
        w.close("c:tx");
    } else if !s.name.is_empty() {
        w.open("c:tx", &[]);
        w.leaf("c:v", &[], &s.name);
        w.close("c:tx");
    }
    match k {
        // Markers only: the series has no line.
        ChartType::Scatter => {
            w.open("c:spPr", &[]);
            w.open("a:ln", &[("w", "19050")]);
            w.empty("a:noFill", &[]);
            w.close("a:ln");
            w.close("c:spPr");
            w.open("c:marker", &[]);
            w.empty("c:symbol", &[("val", "circle")]);
            w.empty("c:size", &[("val", "5")]);
            w.close("c:marker");
        }
        ChartType::Column | ChartType::ColumnStacked | ChartType::Bar | ChartType::BarStacked => {
            w.empty("c:invertIfNegative", &[("val", "0")]);
        }
        ChartType::Line => {
            w.open("c:marker", &[]);
            w.empty("c:symbol", &[("val", "none")]);
            w.close("c:marker");
        }
        _ => {}
    }
    if k == ChartType::Scatter {
        let xs: Option<Vec<Option<f64>>> = spec
            .categories
            .iter()
            .map(|c| if c.is_empty() { Some(None) } else { c.parse::<f64>().ok().filter(|v| v.is_finite()).map(Some) })
            .collect();
        w.open("c:xVal", &[]);
        if embedded {
            let base = index.saturating_mul(3).saturating_add(2);
            numeric_reference(w, column_name(base), spec.categories.len(), &xs.unwrap_or_default());
        } else {
            match xs {
                Some(xs) => num_lit(w, &xs),
                None => str_lit(w, &spec.categories),
            }
        }
        w.close("c:xVal");
        w.open("c:yVal", &[]);
        if embedded {
            let base = index.saturating_mul(3).saturating_add(3);
            numeric_reference(w, column_name(base), spec.categories.len(), &s.values);
        } else {
            num_lit(w, &s.values);
        }
        w.close("c:yVal");
    } else {
        w.open("c:cat", &[]);
        if embedded {
            string_reference(w, "A", &spec.categories);
        } else {
            str_lit(w, &spec.categories);
        }
        w.close("c:cat");
        w.open("c:val", &[]);
        if embedded {
            numeric_reference(w, column_name(index.saturating_add(2)), spec.categories.len(), &s.values);
        } else {
            num_lit(w, &s.values);
        }
        w.close("c:val");
    }
    if matches!(k, ChartType::Line | ChartType::Scatter) {
        w.empty("c:smooth", &[("val", "0")]);
    }
    w.close("c:ser");
}

fn string_reference(w: &mut W, col: &str, values: &[String]) {
    let end = values.len().saturating_add(1);
    w.open("c:strRef", &[]);
    w.leaf("c:f", &[], &format!("Data!${col}$2:${col}${end}"));
    str_cache(w, values);
    w.close("c:strRef");
}

fn str_cache(w: &mut W, values: &[String]) {
    w.open("c:strCache", &[]);
    w.empty("c:ptCount", &[("val", &values.len().to_string())]);
    for (index, value) in values.iter().enumerate() {
        w.open("c:pt", &[("idx", &index.to_string())]);
        w.leaf("c:v", &[], value);
        w.close("c:pt");
    }
    w.close("c:strCache");
}

fn numeric_reference(w: &mut W, col: String, length: usize, values: &[Option<f64>]) {
    let end = length.saturating_add(1);
    w.open("c:numRef", &[]);
    w.leaf("c:f", &[], &format!("Data!${col}$2:${col}${end}"));
    w.open("c:numCache", &[]);
    w.leaf("c:formatCode", &[], "General");
    w.empty("c:ptCount", &[("val", &values.len().to_string())]);
    for (index, value) in values.iter().enumerate() {
        if let Some(value) = value {
            w.open("c:pt", &[("idx", &index.to_string())]);
            w.leaf("c:v", &[], &num(*value));
            w.close("c:pt");
        }
    }
    w.close("c:numCache");
    w.close("c:numRef");
}

fn str_lit(w: &mut W, items: &[String]) {
    w.open("c:strLit", &[]);
    w.empty("c:ptCount", &[("val", &items.len().to_string())]);
    for (i, s) in items.iter().enumerate().filter(|(_, s)| !s.is_empty()) {
        w.open("c:pt", &[("idx", &i.to_string())]);
        w.leaf("c:v", &[], s);
        w.close("c:pt");
    }
    w.close("c:strLit");
}

fn num_lit(w: &mut W, values: &[Option<f64>]) {
    w.open("c:numLit", &[]);
    w.leaf("c:formatCode", &[], "General");
    w.empty("c:ptCount", &[("val", &values.len().to_string())]);
    for (i, v) in values.iter().enumerate() {
        if let Some(v) = v.filter(|v| v.is_finite()) {
            w.open("c:pt", &[("idx", &i.to_string())]);
            w.leaf("c:v", &[], &num(v));
            w.close("c:pt");
        }
    }
    w.close("c:numLit");
}

/// A value as the shortest text that reads back as the same number.
fn num(v: f64) -> String {
    if v == 0.0 { "0".to_string() } else { format!("{v}") }
}

fn category_axis(w: &mut W, pos: &str) {
    w.open("c:catAx", &[]);
    w.empty("c:axId", &[("val", CAT_AX)]);
    w.open("c:scaling", &[]);
    w.empty("c:orientation", &[("val", "minMax")]);
    w.close("c:scaling");
    w.empty("c:delete", &[("val", "0")]);
    w.empty("c:axPos", &[("val", pos)]);
    w.empty("c:majorTickMark", &[("val", "none")]);
    w.empty("c:minorTickMark", &[("val", "none")]);
    w.empty("c:tickLblPos", &[("val", "nextTo")]);
    w.empty("c:crossAx", &[("val", VAL_AX)]);
    w.empty("c:crosses", &[("val", "autoZero")]);
    w.empty("c:auto", &[("val", "1")]);
    w.empty("c:lblAlgn", &[("val", "ctr")]);
    w.empty("c:lblOffset", &[("val", "100")]);
    w.empty("c:noMultiLvlLbl", &[("val", "0")]);
    w.close("c:catAx");
}

/// A value axis: with gridlines and no axis line (`grid`), else with its line and no gridlines.
fn value_axis(w: &mut W, id: &str, cross: &str, pos: &str, grid: bool, between: &str) {
    w.open("c:valAx", &[]);
    w.empty("c:axId", &[("val", id)]);
    w.open("c:scaling", &[]);
    w.empty("c:orientation", &[("val", "minMax")]);
    w.close("c:scaling");
    w.empty("c:delete", &[("val", "0")]);
    w.empty("c:axPos", &[("val", pos)]);
    if grid {
        w.empty("c:majorGridlines", &[]);
    }
    w.empty("c:majorTickMark", &[("val", "none")]);
    w.empty("c:minorTickMark", &[("val", "none")]);
    w.empty("c:tickLblPos", &[("val", "nextTo")]);
    if grid {
        w.open("c:spPr", &[]);
        w.open("a:ln", &[]);
        w.empty("a:noFill", &[]);
        w.close("a:ln");
        w.close("c:spPr");
    }
    w.empty("c:crossAx", &[("val", cross)]);
    w.empty("c:crosses", &[("val", "autoZero")]);
    w.empty("c:crossBetween", &[("val", between)]);
    w.close("c:valAx");
}

/// The chart a chart part holds, when the part is exactly what [`chart_xml`] writes for it (so
/// editing and saving it loses nothing). `None` for any other chart.
pub(crate) fn spec_of(space: &El) -> Option<ChartSpec> {
    let chart = space.child("c:chart")?;
    let plot = chart.child("c:plotArea")?;
    let ty = plot.els().find(|e| e.name.ends_with("Chart"))?;
    let val = |e: &El, n: &str| e.child(n).and_then(|c| c.attr("val")).map(str::to_string);
    let bar = |stacked_kind, clustered_kind| if val(ty, "c:grouping").as_deref() == Some("stacked") { stacked_kind } else { clustered_kind };
    let kind = match ty.name.as_str() {
        "c:barChart" if val(ty, "c:barDir").as_deref() == Some("bar") => bar(ChartType::BarStacked, ChartType::Bar),
        "c:barChart" => bar(ChartType::ColumnStacked, ChartType::Column),
        "c:lineChart" => ChartType::Line,
        "c:areaChart" => ChartType::Area,
        "c:pieChart" => ChartType::Pie,
        "c:doughnutChart" => ChartType::Doughnut,
        "c:scatterChart" => ChartType::Scatter,
        _ => return None,
    };
    let scatter = kind == ChartType::Scatter;
    let sers: Vec<&El> = ty.children("c:ser").take(wordcraft_doc::chart::MAX_SERIES + 1).collect();
    // More than the model holds can't be the same chart: don't build it to find out.
    let count = |s: &&El| s.find("c:ptCount").and_then(|c| c.attr("val")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    let longest = sers.iter().map(count).max().unwrap_or(0);
    if sers.len() > wordcraft_doc::chart::MAX_SERIES
        || longest > wordcraft_doc::chart::MAX_POINTS
        || sers.len().saturating_mul(longest) > wordcraft_doc::chart::MAX_CELLS
    {
        return None;
    }
    let categories = sers.first().and_then(|s| s.child(if scatter { "c:xVal" } else { "c:cat" })).map(literal).unwrap_or_default();
    let series = sers
        .iter()
        .map(|s| ChartSeries {
            name: s
                .child("c:tx")
                .and_then(|t| {
                    t.child("c:v").or_else(|| t.find("c:strCache").and_then(|cache| cache.child("c:pt")).and_then(|point| point.child("c:v")))
                })
                .map(|v| v.text())
                .unwrap_or_default(),
            values: s
                .child(if scatter { "c:yVal" } else { "c:val" })
                .map(literal)
                .unwrap_or_default()
                .iter()
                .map(|v| v.parse::<f64>().ok().filter(|v| v.is_finite()))
                .collect(),
        })
        .collect();
    let title = chart.child("c:title").and_then(|t| t.find("a:t")).map(|t| t.text());
    let legend = chart.child("c:legend").map(|l| match val(l, "c:legendPos").as_deref() {
        Some("l") => LegendPos::Left,
        Some("t") => LegendPos::Top,
        Some("b") => LegendPos::Bottom,
        _ => LegendPos::Right,
    });
    let data_labels = ty.child("c:dLbls").and_then(|d| d.child("c:showVal")).and_then(|v| v.attr("val")) == Some("1");
    let spec = ChartSpec { kind, categories, series, title, legend, data_labels }.sanitized();
    // Editable only when nothing would be lost: writing the model again gives the same part.
    let literal = xml::parse(chart_xml(&spec).as_bytes()).ok()?;
    if same(space, &literal, 0) {
        return Some(spec);
    }
    let embedded = xml::parse(chart_xml_mode(&spec, Some("rId1")).as_bytes()).ok()?;
    same(space, &embedded, 0).then_some(spec)
}

/// The points of a `c:strLit` / `c:numLit` (or a cache) as texts by index, `ptCount` long.
fn literal(e: &El) -> Vec<String> {
    let Some(lit) = e.find("c:strLit").or_else(|| e.find("c:numLit")).or_else(|| e.find("c:strCache")).or_else(|| e.find("c:numCache")) else {
        return Vec::new();
    };
    let n = lit
        .child("c:ptCount")
        .and_then(|c| c.attr("val"))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
        .min(wordcraft_doc::chart::MAX_POINTS + 1);
    let mut out = vec![String::new(); n];
    for p in lit.children("c:pt") {
        if let Some(slot) = p.attr("idx").and_then(|i| i.parse::<usize>().ok()).and_then(|i| out.get_mut(i)) {
            *slot = p.child("c:v").map(|v| v.text()).unwrap_or_default();
        }
    }
    out
}

/// Whether two elements are the same markup: names, attributes (in any order), child elements in
/// order and text (whitespace-only text between elements ignored).
fn same(a: &El, b: &El, depth: usize) -> bool {
    if depth > MAX_DEPTH || a.name != b.name || a.attrs.len() != b.attrs.len() {
        return false;
    }
    if !a.attrs.iter().all(|(k, v)| b.attr(k) == Some(v.as_str()) && b.attrs.iter().any(|(bk, _)| bk == k)) {
        return false;
    }
    let text = |e: &El| -> String {
        e.kids
            .iter()
            .filter_map(|n| match n {
                Node::Text(t) if !t.trim().is_empty() => Some(t.as_str()),
                _ => None,
            })
            .collect()
    };
    if text(a) != text(b) {
        return false;
    }
    let (mut x, mut y) = (a.els(), b.els());
    loop {
        match (x.next(), y.next()) {
            (None, None) => return true,
            (Some(p), Some(q)) if same(p, q, depth + 1) => {}
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wordcraft_doc::THEME_COLORS;

    fn parse(spec: &ChartSpec) -> El {
        xml::parse(chart_xml(spec).as_bytes()).unwrap()
    }

    /// Children of `e` by name, in order.
    fn names(e: &El) -> Vec<&str> {
        e.els().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn every_type_reads_back_as_itself() {
        for kind in ChartType::ALL {
            let mut spec = ChartSpec::sample(kind);
            spec.title = Some("Sales & <costs>".into());
            spec.data_labels = true;
            spec.legend = Some(LegendPos::Right);
            spec.series[0].values[1] = None;
            spec.categories[2] = String::new();
            let spec = spec.sanitized();
            assert_eq!(spec_of(&parse(&spec)).as_ref(), Some(&spec), "{kind:?}");
            assert!(!chart_items(&spec, &THEME_COLORS, 300.0, 200.0).is_empty(), "{kind:?} draws");
        }
        let plain = ChartSpec { legend: None, ..ChartSpec::sample(ChartType::Line) };
        assert_eq!(spec_of(&parse(&plain)), Some(plain));
    }

    #[test]
    fn generated_chart_workbooks_pass_the_editing_mapper_for_supported_families() {
        for kind in ChartType::ALL {
            let mut spec = ChartSpec::sample(kind);
            if kind.is_round() {
                spec.series.truncate(1);
            }
            let (chart, workbook) = chart_package_parts(&spec, "rId1").expect("supported sample chart should produce a valid package pair");
            let rel = Rel {
                id: "rId1".into(),
                kind: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/package".into(),
                target: "word/embeddings/workbook1.xlsx".into(),
                external: false,
            };
            assert!(
                matches!(
                    crate::chart_mapping::map_chart(chart.as_bytes(), &[rel], "word/embeddings/workbook1.xlsx", &workbook),
                    crate::chart_mapping::Classification::Mappable(_)
                ),
                "{kind:?}"
            );
            let parsed = xml::parse(chart.as_bytes()).expect("generated chart XML should parse");
            assert_eq!(spec_of(&parsed).as_ref(), Some(&spec.sanitized()), "{kind:?} model should remain available after reopening");
        }
    }

    #[test]
    fn generated_scatter_workbook_maps_multiple_independent_xy_series() {
        let spec = ChartSpec::sample(ChartType::Scatter);
        let (chart, workbook) = chart_package_parts(&spec, "rId1").expect("sample scatter should be supported");
        let rel = Rel {
            id: "rId1".into(),
            kind: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/package".into(),
            target: "word/embeddings/workbook1.xlsx".into(),
            external: false,
        };
        let result = crate::chart_mapping::map_chart(chart.as_bytes(), &[rel], "word/embeddings/workbook1.xlsx", &workbook);
        assert!(matches!(result, crate::chart_mapping::Classification::Mappable(_)), "generated scatter chart should map: {result:?}");
        let crate::chart_mapping::Classification::Mappable(mapping) = result else { return };
        assert_eq!(mapping.series.len(), 2);
        assert!(matches!(mapping.series[0].data, crate::chart_mapping::SeriesDataMapping::Scatter { .. }));
    }

    #[test]
    fn sparse_chart_keeps_a_valid_workbook_but_remains_read_only() {
        let mut gaps = ChartSpec::sample(ChartType::Column);
        gaps.series[0].values[1] = None;
        let (chart, workbook) = chart_package_parts(&gaps, "rId1").expect("a chart with a gap still gets a valid workbook");
        let rel = Rel {
            id: "rId1".into(),
            kind: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/package".into(),
            target: "word/embeddings/workbook1.xlsx".into(),
            external: false,
        };
        assert!(matches!(
            crate::chart_mapping::map_chart(chart.as_bytes(), &[rel], "word/embeddings/workbook1.xlsx", &workbook),
            crate::chart_mapping::Classification::Inconsistent(reason) if reason.contains("references a missing")
        ));

        let mut pie = ChartSpec::sample(ChartType::Pie);
        pie.series.push(pie.series[0].clone());
        assert!(chart_package_parts(&pie, "rId1").is_err());
    }

    #[test]
    fn inserted_chart_round_trips_with_an_editable_package_relationship() {
        use std::sync::Arc;
        use wordcraft_doc::graphic::{Graphic, GraphicKind};
        use wordcraft_doc::para::{Float, InlineObject, Paragraph};
        use wordcraft_doc::{Block, Document};

        let spec = ChartSpec::sample(ChartType::Column);
        let graphic = Graphic {
            kind: GraphicKind::Chart,
            items: chart_items(&spec, &THEME_COLORS, 360.0, 220.0),
            w: 360.0,
            h: 220.0,
            source: None,
            smart_art: None,
            chart: Some(Arc::new(spec)),
        };
        let mut paragraph = Paragraph::new();
        paragraph
            .insert_object(
                0,
                InlineObject::Graphic { w: 360.0, h: 220.0, alt: String::new(), float: Float::default(), graphic: Arc::new(graphic) },
                &Default::default(),
            )
            .expect("chart object should fit in the new paragraph");
        let mut document = Document::new();
        document.body = vec![Arc::new(Block::Para(paragraph))];

        let docx = crate::write::write(&document).expect("inserted chart should be written with its workbook");
        let reopened = crate::read::read(&docx).expect("chart DOCX should reopen");
        assert!(
            crate::chart_mapping::inspect_document(&reopened)
                .iter()
                .all(|classification| matches!(classification, crate::chart_mapping::Classification::Mappable(_)))
        );
        assert!(crate::chart_mapping::document_has_chart(&reopened, "word/charts/chart1.xml"));
        let plan = crate::chart_mapping::prepare_chart_edit(&docx, "word/charts/chart1.xml").expect("inserted chart should get an edit plan");
        let updated = crate::chart_mapping::apply_chart_edit(
            &docx,
            &plan,
            &crate::chart_mapping::ChartDataChanges {
                series: vec![
                    crate::chart_mapping::SeriesChanges { values: Some(vec![8.0, 9.0, 10.0, 11.0]), ..Default::default() },
                    Default::default(),
                    Default::default(),
                ],
                ..Default::default()
            },
        )
        .expect("the existing transaction should edit an inserted chart");
        let snapshot = crate::chart_mapping::inspect_chart_data(&updated, "word/charts/chart1.xml").expect("edited chart should remain mappable");
        assert_eq!(snapshot.series[0].values, vec![8.0, 9.0, 10.0, 11.0]);
        assert!(
            crate::chart_mapping::inspect_document(&crate::read::read(&updated).expect("edited chart package should reopen"))
                .iter()
                .all(|classification| matches!(classification, crate::chart_mapping::Classification::Mappable(_)))
        );
    }

    #[test]
    fn inserted_scatter_chart_has_editable_markers_through_the_package_mapper() {
        use std::sync::Arc;
        use wordcraft_doc::graphic::{Graphic, GraphicKind};
        use wordcraft_doc::para::{Float, InlineObject, Paragraph};
        use wordcraft_doc::{Block, Document};

        let spec = ChartSpec::sample(ChartType::Scatter);
        let graphic = Graphic {
            kind: GraphicKind::Chart,
            items: chart_items(&spec, &THEME_COLORS, 360.0, 220.0),
            w: 360.0,
            h: 220.0,
            source: None,
            smart_art: None,
            chart: Some(Arc::new(spec)),
        };
        let mut paragraph = Paragraph::new();
        paragraph
            .insert_object(
                0,
                InlineObject::Graphic { w: 360.0, h: 220.0, alt: String::new(), float: Float::default(), graphic: Arc::new(graphic) },
                &Default::default(),
            )
            .expect("new scatter chart should fit");
        let mut document = Document::new();
        document.body = vec![Arc::new(Block::Para(paragraph))];
        let source = crate::write::write(&document).expect("scatter chart should be packaged with data");
        let original = crate::chart_mapping::inspect_chart_data(&source, "word/charts/chart1.xml").expect("inserted chart should map");
        assert!(original.series.iter().all(|series| series.marker_symbol_editable && series.marker_size_editable));
        assert!(original.series.iter().all(|series| series.marker_symbol.as_deref() == Some("circle")));
        let plan = crate::chart_mapping::prepare_chart_edit(&source, "word/charts/chart1.xml").expect("marker edits should have a validated plan");
        let updated = crate::chart_mapping::apply_chart_edit(
            &source,
            &plan,
            &crate::chart_mapping::ChartDataChanges {
                series: vec![
                    crate::chart_mapping::SeriesChanges { marker_symbol: Some("diamond".into()), marker_size: Some(10), ..Default::default() },
                    crate::chart_mapping::SeriesChanges { marker_symbol: Some("square".into()), marker_size: Some(8), ..Default::default() },
                ],
                ..Default::default()
            },
        )
        .expect("inserted scatter marker formatting should transact");
        let snapshot = crate::chart_mapping::inspect_chart_data(&updated, "word/charts/chart1.xml").expect("edited chart should remap");
        assert_eq!(snapshot.series[0].marker_symbol.as_deref(), Some("diamond"));
        assert_eq!(snapshot.series[0].marker_size, Some(10));
        assert_eq!(snapshot.series[1].marker_symbol.as_deref(), Some("square"));
        assert_eq!(snapshot.series[1].marker_size, Some(8));
        assert!(crate::read::read(&updated).is_ok(), "updated chart package should reopen");
    }

    #[test]
    fn a_chart_with_more_than_the_model_stays_read_only() {
        let spec = ChartSpec::sample(ChartType::Column);
        let x = chart_xml(&spec).replace("<c:roundedCorners val=\"0\"/>", "<c:roundedCorners val=\"0\"/><c:style val=\"2\"/>");
        assert_eq!(spec_of(&xml::parse(x.as_bytes()).unwrap()), None);
        let x = chart_xml(&spec).replace("<c:v>3.2</c:v>", "<c:v>3.20</c:v>");
        assert_eq!(spec_of(&xml::parse(x.as_bytes()).unwrap()), None);
    }

    #[test]
    fn elements_come_in_schema_order() {
        let mut spec = ChartSpec::sample(ChartType::ColumnStacked);
        spec.title = Some("T".into());
        spec.data_labels = true;
        let space = parse(&spec);
        assert_eq!(names(&space), ["c:roundedCorners", "c:chart", "c:spPr"]);
        let chart = space.child("c:chart").unwrap();
        assert_eq!(names(chart), ["c:title", "c:autoTitleDeleted", "c:plotArea", "c:legend", "c:plotVisOnly", "c:dispBlanksAs"]);
        let plot = chart.child("c:plotArea").unwrap();
        assert_eq!(names(plot), ["c:layout", "c:barChart", "c:catAx", "c:valAx"]);
        let bar = plot.child("c:barChart").unwrap();
        assert_eq!(
            names(bar),
            ["c:barDir", "c:grouping", "c:varyColors", "c:ser", "c:ser", "c:ser", "c:dLbls", "c:gapWidth", "c:overlap", "c:axId", "c:axId"]
        );
        assert_eq!(names(bar.child("c:ser").unwrap()), ["c:idx", "c:order", "c:tx", "c:invertIfNegative", "c:cat", "c:val"]);
        let val_ax = plot.child("c:valAx").unwrap();
        assert_eq!(
            names(val_ax),
            [
                "c:axId",
                "c:scaling",
                "c:delete",
                "c:axPos",
                "c:majorGridlines",
                "c:majorTickMark",
                "c:minorTickMark",
                "c:tickLblPos",
                "c:spPr",
                "c:crossAx",
                "c:crosses",
                "c:crossBetween"
            ]
        );
        let line = parse(&ChartSpec { data_labels: true, ..ChartSpec::sample(ChartType::Line) });
        let lc = line.child("c:chart").unwrap().child("c:plotArea").unwrap().child("c:lineChart").unwrap();
        assert_eq!(names(lc), ["c:grouping", "c:varyColors", "c:ser", "c:ser", "c:ser", "c:dLbls", "c:marker", "c:axId", "c:axId"]);
        assert_eq!(names(lc.child("c:ser").unwrap()), ["c:idx", "c:order", "c:tx", "c:marker", "c:cat", "c:val", "c:smooth"]);
        let sc = parse(&ChartSpec::sample(ChartType::Scatter));
        let sc = sc.child("c:chart").unwrap().child("c:plotArea").unwrap().child("c:scatterChart").unwrap();
        assert_eq!(names(sc.child("c:ser").unwrap()), ["c:idx", "c:order", "c:tx", "c:spPr", "c:marker", "c:xVal", "c:yVal", "c:smooth"]);
        let dn = parse(&ChartSpec::sample(ChartType::Doughnut));
        let dn = dn.child("c:chart").unwrap().child("c:plotArea").unwrap();
        assert_eq!(names(dn), ["c:layout", "c:doughnutChart"]);
        assert_eq!(names(dn.child("c:doughnutChart").unwrap()), ["c:varyColors", "c:ser", "c:firstSliceAng", "c:holeSize"]);
    }
}
