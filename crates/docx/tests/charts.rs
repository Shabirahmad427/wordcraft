//! Synthetic chart round-trip integration regressions.
#[path = "common/charts.rs"]
mod fixtures;
use fixtures::*;
use wordcraft_doc::{InlineObject, PartKind, StoryRef};
const OFFICE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
#[test]
fn scientific_thesis_and_tracked_report_charts_survive_three_generations() {
    for kind in ["scientific", "thesis", "tracked report"] {
        let input = fixture(kind);
        if let Some(directory) = std::env::var_os("WORDCRAFT_COMPAT_CORPUS_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join(format!("{}.docx", kind.replace(' ', "-"))), archive(&input)).unwrap();
        }
        let mut doc = wordcraft_docx::read(&archive(&input)).unwrap();
        assert!(doc.plain_text(StoryRef::Body).contains("Chart preserved"));
        assert!(
            doc.body
                .iter()
                .filter_map(|b| b.as_para())
                .flat_map(|p| &p.objects)
                .any(|o| matches!(o, InlineObject::Opaque { format, .. } if format == "docx-chart"))
        );
        doc.insert_text(&wordcraft_doc::Pos::body(0, 0), "Edited: ", &Default::default()).unwrap();
        for flavor in [wordcraft_docx::Flavor::Document, wordcraft_docx::Flavor::Template, wordcraft_docx::Flavor::MacroDocument] {
            let mut current = doc.clone();
            for _ in 0..3 {
                let saved = wordcraft_docx::write_as(&current, flavor).unwrap();
                let output = unpack(&saved);
                for path in ["word/charts/chart1.xml", "word/charts/_rels/chart1.xml.rels", "word/charts/style1.xml", "word/embeddings/data.xlsx"] {
                    assert_eq!(output.get(path), input.get(path), "lost or changed {path} ({kind})");
                }
                assert!(text(&output, "word/_rels/document.xml.rels").contains("/word/charts/chart1.xml"));
                let body = text(&output, "word/document.xml");
                assert!(body.contains("c:chart") && body.contains("wcChart"));
                assert!(!body.contains("Chart preserved"));
                assert!(body.contains("ZOTERO_ITEM") && body.contains("CSL_CITATION") && body.contains("REF Figure1") && body.contains("m:oMath"));
                assert!(text(&output, "[Content_Types].xml").contains("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"));
                if kind == "tracked report" {
                    assert!(body.contains("Tracked addition") && body.contains("w:ins"));
                }
                current = wordcraft_docx::read(&saved).unwrap();
                assert!(current.plain_text(StoryRef::Body).starts_with("Edited: "));
                assert_eq!(current.custom_prop("ZOTERO_PREF_1"), doc.custom_prop("ZOTERO_PREF_1"));
                assert!(current.parts.values().any(|p| p.kind == PartKind::Footnote));
            }
        }
    }
}
#[test]
fn story_local_ids_are_rebound_for_body_and_header() {
    let mut files = fixture("scientific");
    let header = files.keys().find(|n| n.starts_with("word/header") && n.ends_with(".xml")).unwrap().clone();
    replace(&mut files, &header, "</w:p>", &format!("{}</w:p>", drawing("originalChart", false)));
    let filename = header.strip_prefix("word/").unwrap();
    files.insert(
        format!("word/_rels/{filename}.rels"),
        format!(
            r#"<Relationships xmlns="{REL_NS}"><Relationship Id="originalChart" Type="{OFFICE}chart" Target="charts/chart2.xml"/></Relationships>"#
        )
        .into_bytes(),
    );
    files.insert(
        "word/charts/chart2.xml".into(),
        CHART.replace(r#"<c:externalData r:id="book"><c:autoUpdate val="0"/></c:externalData>"#, "").into_bytes(),
    );
    replace(
        &mut files,
        "[Content_Types].xml",
        "</Types>",
        r#"<Override PartName="/word/charts/chart2.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/></Types>"#,
    );
    let doc = wordcraft_docx::read(&archive(&files)).unwrap();
    let saved = unpack(&wordcraft_docx::write(&doc).unwrap());
    let header_out = saved.keys().find(|n| n.starts_with("word/header") && n.ends_with(".xml")).unwrap();
    let rels_out = format!("word/_rels/{}.rels", header_out.strip_prefix("word/").unwrap());
    assert!(text(&saved, &rels_out).contains("/word/charts/chart2.xml"));
    assert!(text(&saved, "word/_rels/document.xml.rels").contains("/word/charts/chart1.xml"));
    assert!(wordcraft_docx::read(&archive(&saved)).is_ok());
}
#[test]
fn missing_corrupt_or_unsafe_dependencies_fail_instead_of_disappearing() {
    for case in ["missing workbook", "bad relationships", "reserved target", "unknown namespace", "missing chart ID"] {
        let mut files = fixture("scientific");
        match case {
            "missing workbook" => {
                files.remove("word/embeddings/data.xlsx");
            }
            "bad relationships" => {
                files.insert("word/charts/_rels/chart1.xml.rels".into(), b"<broken".to_vec());
            }
            "reserved target" => replace(&mut files, "word/charts/_rels/chart1.xml.rels", "../embeddings/data.xlsx", "../document.xml"),
            "unknown namespace" => {
                replace(&mut files, "word/document.xml", "<wp:extent", "<wp:extent xmlns:future='urn:unknown' future:unknown='keep'")
            }
            _ => replace(&mut files, "word/document.xml", "originalChart", "notARealRelationship"),
        }
        assert!(wordcraft_docx::read(&archive(&files)).is_err(), "silent data loss: {case}");
    }
}
#[test]
fn missing_malformed_and_colliding_preserved_data_blocks_export() {
    let bytes = archive(&fixture("scientific"));
    let doc = wordcraft_docx::read(&bytes).unwrap();
    let mut missing = doc.clone();
    missing.passthrough.remove("word/embeddings/data.xlsx");
    assert!(wordcraft_docx::write(&missing).is_err());
    let mut corrupt = doc.clone();
    corrupt.passthrough.insert("wordcraft:opaqueCharts.v1".into(), std::sync::Arc::new(b"{bad".to_vec()));
    assert!(wordcraft_docx::write(&corrupt).is_err());
    let mut oversized = doc.clone();
    oversized.passthrough.insert("wordcraft:opaqueCharts.v1".into(), std::sync::Arc::new(vec![b' '; 128 * 1024 + 1]));
    assert!(wordcraft_docx::write(&oversized).is_err());
}

#[test]
fn generated_part_collision_blocks_save_and_chart_ids_remain_unique() {
    let mut files = fixture("scientific");
    replace(&mut files, "word/charts/_rels/chart1.xml.rels", "style1.xml", "../theme/theme1.xml");
    let doc = wordcraft_docx::read(&archive(&files)).unwrap();
    assert!(wordcraft_docx::write(&doc).is_err());
    let mut files = fixture("scientific");
    replace(&mut files, "word/document.xml", "</w:p>", &format!("{}</w:p>", drawing("originalChart", false)));
    let doc = wordcraft_docx::read(&archive(&files)).unwrap();
    let saved = unpack(&wordcraft_docx::write(&doc).unwrap());
    let body = text(&saved, "word/document.xml");
    let ids: Vec<_> = body.split("<wp:docPr ").skip(1).filter_map(|s| s.split("id=\"").nth(1).and_then(|s| s.split('"').next())).collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids.first(), ids.get(1));
}

#[test]
fn malformed_root_relationships_and_unsupported_drawing_types_are_reported() {
    let mut files = fixture("scientific");
    files.insert("word/_rels/document.xml.rels".into(), b"<broken".to_vec());
    assert!(wordcraft_docx::read(&archive(&files)).is_err());
    let mut files = fixture("scientific");
    replace(&mut files, "word/charts/_rels/chart1.xml.rels", " Id=\"style\"", " Id=\"book\"");
    assert!(wordcraft_docx::read(&archive(&files)).is_err());
    let mut files = fixture("scientific");
    replace(
        &mut files,
        "word/document.xml",
        "<c:chart xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" r:id=\"originalChart\"/>",
        "<unknown:diagram xmlns:unknown='urn:unknown' r:id='originalChart'/>",
    );
    assert!(wordcraft_docx::read(&archive(&files)).is_err());
}
