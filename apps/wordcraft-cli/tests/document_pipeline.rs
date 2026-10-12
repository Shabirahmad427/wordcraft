//! End-to-end checks through the shipped CLI entry points using synthetic DOCX inputs.

use std::{path::PathBuf, process::Command};

use wordcraft_engine::doc::{Document, Paragraph, PartKind, Pos, TextBody, para::InlineObject, para::ShapeKind, para_block, props::TextVert};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("wordcraft-cli-pipeline-{}-{}", std::process::id(), wordcraft_engine::recovery::unique_id()));
        std::fs::create_dir(&path).expect("create scratch directory");
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wordcraft-cli"))
}

fn assert_success(output: std::process::Output, step: &str) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "CLI {step} failed: {stderr}");
    stdout
}

#[test]
fn cli_edit_docx_roundtrip_and_pdf_export_preserve_image_text_and_pages() {
    let scratch = Scratch::new();
    let source = scratch.path("source.docx");
    let edited = scratch.path("edited.docx");
    let pdf = scratch.path("edited.pdf");
    let mut document = Document::new();
    document.body.push(para_block(Paragraph::with_text("Pipeline title", Default::default())));
    for index in 1..25 {
        document.body.push(para_block(Paragraph::with_text(&format!("filler {index}"), Default::default())));
    }
    let mut heading = Paragraph::with_text("HEADING", Default::default());
    heading.props.keep_next = Some(true);
    document.body.push(para_block(heading));
    let body_text = format!("BODYSTART {}", std::iter::repeat_n("body sentence", 60).collect::<Vec<_>>().join(" "));
    let mut body = Paragraph::with_text(&body_text, Default::default());
    body.props.widow_control = Some(true);
    document.body.push(para_block(body));

    let box_id = document.add_part(PartKind::TextBox, vec![para_block(Paragraph::with_text("Vertical label", Default::default()))]);
    if let Some(part) = document.parts.get_mut(&box_id) {
        part.body = TextBody { vert: TextVert::Vert, ..Default::default() };
    }
    document
        .insert_object(
            &Pos::body(25, 0),
            InlineObject::Shape {
                kind: ShapeKind::TextBox,
                w: 72.0,
                h: 36.0,
                fill: None,
                stroke: None,
                stroke_width: 0.75,
                float: Default::default(),
                story: Some(box_id),
                effects: Default::default(),
                freeform: None,
                extra: Default::default(),
            },
            &Default::default(),
        )
        .expect("insert synthetic vertical text box");

    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(12, 8, image::Rgba([30, 120, 210, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("encode synthetic fixture image");
    let image = document.add_media(png.into_inner(), "png");
    document
        .insert_object(
            &Pos::body(0, 0),
            InlineObject::Image {
                media: image,
                w: 36.0,
                h: 24.0,
                alt: "Synthetic pipeline test image".into(),
                float: Default::default(),
                crop: [0.0; 4],
                ole: None,
            },
            &Default::default(),
        )
        .expect("insert synthetic image");
    let source_bytes = wordcraft_engine::io::save_bytes("source.docx", &document).expect("write source fixture");
    std::fs::write(&source, &source_bytes).expect("save source fixture");

    let output = cli()
        .args([
            "run",
            "--file",
            source.to_str().expect("UTF-8 temp path"),
            "--cmd",
            "caret.docEnd",
            "--cmd",
            r#"text.insert={"text":" CLI edit survived."}"#,
            "--save",
            edited.to_str().expect("UTF-8 temp path"),
        ])
        .output()
        .expect("run CLI editing workflow");
    assert_success(output, "DOCX edit and save");
    assert_eq!(std::fs::read(&source).expect("source remains readable"), source_bytes, "editing must not replace the input file");

    let text = assert_success(
        cli().args(["text", edited.to_str().expect("UTF-8 temp path")]).output().expect("run CLI text extraction"),
        "reopen and inspect edited DOCX",
    );
    assert!(text.contains("Pipeline title"));
    assert!(text.contains("CLI edit survived."));
    assert_eq!(text.matches("HEADING").count(), 1, "pagination must not duplicate the heading");
    assert_eq!(text.matches("BODYSTART").count(), 1, "pagination must not drop or duplicate the following paragraph");
    let reopened = wordcraft_engine::io::open_path(&edited).expect("open edited DOCX directly");
    let image_count = reopened
        .body
        .iter()
        .filter_map(|block| block.as_para())
        .flat_map(|paragraph| paragraph.objects.iter())
        .filter(|object| matches!(object, InlineObject::Image { .. }))
        .count();
    assert_eq!(image_count, 1, "CLI text editing and DOCX save/reopen must retain the inserted image");
    let vertical_box = reopened.parts.values().find(|part| part.kind == PartKind::TextBox && part.body.vert == TextVert::Vert);
    assert!(vertical_box.is_some(), "CLI round-trip must retain vertical text-box direction");

    let output = cli()
        .args(["convert", edited.to_str().expect("UTF-8 temp path"), pdf.to_str().expect("UTF-8 temp path")])
        .output()
        .expect("run CLI PDF conversion");
    assert_success(output, "PDF conversion");
    let pdf_bytes = std::fs::read(&pdf).expect("read generated PDF");
    assert!(pdf_bytes.starts_with(b"%PDF-"), "CLI must produce a PDF document");
    let pdf_source = String::from_utf8_lossy(&pdf_bytes);
    assert!(pdf_source.contains("/Subtype/Image"), "image should be emitted as a PDF image XObject");

    let info =
        assert_success(cli().args(["info", edited.to_str().expect("UTF-8 temp path")]).output().expect("run CLI info"), "pagination inspection");
    let info: serde_json::Value = serde_json::from_str(&info).expect("CLI info is JSON");
    assert_eq!(info.get("pages").and_then(serde_json::Value::as_u64), Some(2), "fixture should paginate into two pages");
}
