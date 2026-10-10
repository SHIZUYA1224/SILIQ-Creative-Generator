use design_export::{ExportFormat, ExportRecipe, export};
use design_render::{render, resources::ResourceStore};
use serde_json::json;
use std::path::Path;
fn output() -> design_render::RenderOutput {
    let d = json!({"format":"siliq-creative-generator-core","schemaVersion":"0.4.0-core","documentId":"export-test","revision":0,"title":"public-safe","colorSpace":"sRGB","baseArtboard":{"width":32,"height":32,"rootNodeId":"root"},"nodes":[{"id":"root","name":"root","type":"group","bounds":{"x":0,"y":0,"width":32,"height":32},"rotationDeg":0,"opacity":1,"visible":true,"locked":false,"childIds":["shape"]},{"id":"shape","name":"shape","type":"rect","bounds":{"x":4,"y":4,"width":24,"height":24},"rotationDeg":0,"opacity":0.5,"visible":true,"locked":false,"fill":{"kind":"solid","color":"#ff0000"},"cornerRadius":0}],"fonts":[],"assets":[],"slots":[],"variants":[{"id":"main","label":"main","width":32,"height":32,"approvalStatus":"approved","nodePatches":[]}]});
    let store = ResourceStore::open(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/public-safe/resources"
    )))
    .unwrap();
    render(d.to_string().as_bytes(), "main", &store).unwrap()
}
#[test]
fn derivative_formats_are_explicit_and_real_files() {
    let raster = output();
    let png = export(&raster, &ExportRecipe::default()).unwrap();
    assert_eq!(png.bytes, raster.png);
    let jpg = export(
        &raster,
        &ExportRecipe {
            format: ExportFormat::Jpeg,
            quality: Some(92),
            jpeg_background: Some("#ffffff".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(jpg.bytes.starts_with(&[0xff, 0xd8]));
    assert_eq!(jpg.report.alpha, "flattened");
    let webp = export(
        &raster,
        &ExportRecipe {
            format: ExportFormat::Webp,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(&webp.bytes[0..4], b"RIFF");
    assert_eq!(&webp.bytes[8..12], b"WEBP");
    let svg = export(
        &raster,
        &ExportRecipe {
            format: ExportFormat::Svg,
            allow_rasterization: true,
            ..Default::default()
        },
    )
    .unwrap();
    let s = String::from_utf8(svg.bytes).unwrap();
    assert!(s.contains("data:image/png;base64,"));
    assert!(svg.report.rasterized && !svg.report.text_editable);
    let pdf = export(
        &raster,
        &ExportRecipe {
            format: ExportFormat::Pdf,
            allow_rasterization: true,
            pdf_dpi: 144,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(pdf.bytes.starts_with(b"%PDF-"));
    assert!(pdf.bytes.windows(5).any(|w| w == b"%%EOF"));
    assert_eq!(pdf.report.rasterization_dpi, Some(144));
}
#[test]
fn destructive_conversion_requires_approval_and_settings_are_not_ignored() {
    let out = output();
    assert!(
        export(
            &out,
            &ExportRecipe {
                format: ExportFormat::Pdf,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        export(
            &out,
            &ExportRecipe {
                format: ExportFormat::Svg,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        export(
            &out,
            &ExportRecipe {
                format: ExportFormat::Jpeg,
                quality: Some(90),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        export(
            &out,
            &ExportRecipe {
                format: ExportFormat::Webp,
                webp_lossless: false,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        export(
            &out,
            &ExportRecipe {
                format: ExportFormat::Png,
                quality: Some(90),
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn lossless_webp_roundtrip_preserves_straight_alpha_pixels() {
    let out = output();
    let webp = export(
        &out,
        &ExportRecipe {
            format: ExportFormat::Webp,
            ..Default::default()
        },
    )
    .unwrap();
    let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(webp.bytes)).unwrap();
    assert_eq!(decoder.dimensions(), (32, 32));
    let mut rgba = vec![0; decoder.output_buffer_size().unwrap()];
    decoder.read_image(&mut rgba).unwrap();
    assert_eq!(rgba, out.rgba);
}
