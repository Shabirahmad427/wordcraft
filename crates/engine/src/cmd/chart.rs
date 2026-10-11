//! Agent-facing operations for safely editing supported embedded chart data.

use serde_json::{Value, json};

use crate::{CmdError, CmdResult, CommandSpec, Session, p};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("chart.editData", "Edit Chart Data", "Agents", edit_data)
            .params(r#"{"documentId": integer, "chartPart": string, "series": [{"name"?: string, "categories"?: [string], "values"?: [number]}]}"#),
    ]
}

fn edit_data(session: &mut Session, params: &Value) -> CmdResult {
    let document_id =
        params.get("documentId").and_then(Value::as_u64).ok_or_else(|| CmdError::Params("`documentId` (integer) is required".into()))?;
    if document_id != session.document_id() {
        return Err(CmdError::Failed("stale chart edit: the open document has changed".into()));
    }
    if session.source_package.is_none() {
        return Err(CmdError::Disabled("chart editing requires an opened, unencrypted DOCX package".into()));
    }
    let chart_part = p::req_str(params, "chartPart")?;
    if !wordcraft_docx::document_has_chart(&session.doc, chart_part) {
        return Err(CmdError::Failed(format!("chart `{chart_part}` is not part of the current document")));
    }
    let raw_changes = params.get("series").cloned().ok_or_else(|| CmdError::Params("`series` (array) is required".into()))?;
    let changes: wordcraft_docx::ChartDataChanges =
        serde_json::from_value(json!({"series": raw_changes})).map_err(|error| CmdError::Params(format!("invalid chart changes: {error}")))?;

    // Serialize the current guarded editor projection first, so any supported edits already in
    // the session are included in the transaction's exact package digest and preserved baseline.
    let current_package = wordcraft_docx::write(&session.doc).map_err(|error| CmdError::Failed(error.to_string()))?;
    let plan = wordcraft_docx::prepare_chart_edit(&current_package, chart_part).map_err(|error| CmdError::Failed(error.to_string()))?;
    let updated_package = wordcraft_docx::apply_chart_edit(&current_package, &plan, &changes).map_err(|error| CmdError::Failed(error.to_string()))?;

    // Re-import the validated package as one model replacement. This refreshes chart cache
    // rendering and makes the synchronized chart/workbook bytes the new guarded save baseline.
    let updated_document = wordcraft_docx::read(&updated_package).map_err(|error| CmdError::Failed(error.to_string()))?;
    if !wordcraft_docx::document_has_chart(&updated_document, chart_part) {
        return Err(CmdError::Failed("updated DOCX no longer contains the edited chart".into()));
    }
    session.doc = updated_document;
    // `doc.passthrough` now owns the updated guarded package baseline. Keep SourcePackage as the
    // exact file originally opened so recovery's “Save Original Package” remains exact.
    session.relayout();
    session.clamp_selection();
    session.close_typing();
    Ok(json!({"chartPart": chart_part, "documentId": document_id, "updated": true}))
}
