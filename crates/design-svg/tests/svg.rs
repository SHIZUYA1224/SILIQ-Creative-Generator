//! Safe-subset SVG import: the fixture logo is accepted; everything outside
//! the allowlist is refused with an explicit error.

use design_svg::*;

const LOGO: &[u8] =
    include_bytes!("../../../fixtures/public-safe/resources/assets/08317af1678409f3a1dcf29b651f12b439d311bcddefdffd3580cd4b0067c96c.svg");

fn svg(body: &str) -> String {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">{body}</svg>"#)
}

fn kind(src: &str) -> SvgErrorKind {
    match import_svg(src.as_bytes()) {
        Ok(_) => panic!("expected rejection for {src}"),
        Err(e) => e.kind,
    }
}

#[test]
fn fixture_logo_is_accepted() {
    let art = import_svg(LOGO).unwrap();
    assert_eq!(
        art.view_box,
        ViewBox {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 100.0
        }
    );
    // mark (gradient, even-odd), dot, 3 bars/polygon, ellipse
    assert_eq!(art.shapes.len(), 6);
    assert!(matches!(art.shapes[0].fill, Fill::Linear { .. }));
    assert_eq!(art.shapes[0].fill_rule, FillRule::EvenOdd);
    assert!((art.shapes[3].opacity - 0.7).abs() < 1e-12);
}

#[test]
fn basic_shapes_and_transforms() {
    let art = import_svg(svg(r##"<g transform="translate(10 20) scale(2)"><rect width="5" height="5" fill="#ff0000"/></g>"##).as_bytes())
        .unwrap();
    let Segment::MoveTo(p) = art.shapes[0].segments[0] else { panic!() };
    assert_eq!((p.x, p.y), (10.0, 20.0));
    let Segment::LineTo(p) = art.shapes[0].segments[2] else { panic!() };
    assert_eq!((p.x, p.y), (20.0, 30.0));
    assert_eq!(art.shapes[0].fill, Fill::Solid { rgb: [255, 0, 0] });
}

#[test]
fn relative_and_smooth_path_commands() {
    let art = import_svg(svg(r#"<path d="m10 10 l10 0 v10 h-10 z M0 0 c1 1 2 2 3 3 s4 4 5 5 q1 1 2 2 t3 3"/>"#).as_bytes()).unwrap();
    let segs = &art.shapes[0].segments;
    assert_eq!(segs[1], Segment::LineTo(Point { x: 20.0, y: 10.0 }));
    assert_eq!(segs[2], Segment::LineTo(Point { x: 20.0, y: 20.0 }));
    assert_eq!(segs[3], Segment::LineTo(Point { x: 10.0, y: 20.0 }));
    // s reflects the previous control point (2,2) about (3,3) → (4,4)
    let Segment::CubicTo(c1, _, end) = segs[7] else {
        panic!("{segs:?}")
    };
    assert_eq!((c1.x, c1.y, end.x, end.y), (4.0, 4.0, 8.0, 8.0));
}

#[test]
fn bounding_box_gradient_uses_exact_curve_bounds() {
    // Circle r=10 at (50,50): exact bbox is 40..60; the control polygon of the
    // Bézier approximation would also be 40..60 here, so use a curve whose
    // control points overshoot: the cubic bulges to y=7.5 while controls are at 0.
    let art = import_svg(
        svg(r##"<defs><linearGradient id="g"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#fff"/></linearGradient></defs><path fill="url(#g)" d="M0 10 C0 0 10 0 10 10 Z"/>"##).as_bytes(),
    )
    .unwrap();
    let Fill::Linear { matrix, .. } = &art.shapes[0].fill else {
        panic!()
    };
    // bbox: x 0..10, y 2.5..10  (cubic minimum y = 10 - 0.75*10 = 2.5)
    assert!((matrix.f - 2.5).abs() < 1e-9, "{matrix:?}");
    assert!((matrix.d - 7.5).abs() < 1e-9, "{matrix:?}");
}

#[test]
fn script_rejected() {
    assert_eq!(kind(&svg("<script>alert(1)</script>")), SvgErrorKind::Unsupported);
}

#[test]
fn foreign_object_rejected() {
    assert_eq!(kind(&svg("<foreignObject width=\"10\" height=\"10\"/>")), SvgErrorKind::Unsupported);
}

#[test]
fn event_handler_rejected() {
    assert_eq!(
        kind(&svg(r#"<rect width="1" height="1" onclick="x()"/>"#)),
        SvgErrorKind::Unsupported
    );
    let root = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1" onload="x()"/>"#;
    assert_eq!(kind(root), SvgErrorKind::Unsupported);
}

#[test]
fn external_image_reference_rejected() {
    let s = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 1 1"><image xlink:href="https://example.com/a.png"/></svg>"#;
    assert_eq!(kind(s), SvgErrorKind::ExternalReference);
}

#[test]
fn external_fill_url_rejected() {
    assert_eq!(
        kind(&svg(r#"<rect width="1" height="1" fill="url(https://example.com/x.svg#g)"/>"#)),
        SvgErrorKind::ExternalReference
    );
}

#[test]
fn internal_use_reference_rejected() {
    let s = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 1 1"><use xlink:href="#a"/></svg>"##;
    assert_eq!(kind(s), SvgErrorKind::Unsupported);
}

#[test]
fn dtd_and_entities_rejected() {
    let s = r#"<?xml version="1.0"?><!DOCTYPE svg [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;">]><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><title>&b;</title></svg>"#;
    assert_eq!(kind(s), SvgErrorKind::Unsupported);
}

#[test]
fn css_rejected() {
    assert_eq!(
        kind(&svg(r#"<rect width="1" height="1" style="fill:red"/>"#)),
        SvgErrorKind::Unsupported
    );
    assert_eq!(kind(&svg("<style>rect{fill:red}</style>")), SvgErrorKind::Unsupported);
    assert_eq!(kind(&svg(r#"<rect width="1" height="1" class="a"/>"#)), SvgErrorKind::Unsupported);
}

#[test]
fn stylesheet_processing_instruction_rejected() {
    let s = r#"<?xml-stylesheet href="https://example.com/a.css"?><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"/>"#;
    assert_eq!(kind(s), SvgErrorKind::Unsupported);
}

#[test]
fn text_rejected() {
    assert_eq!(kind(&svg("<text>SILIQ</text>")), SvgErrorKind::Unsupported);
}

#[test]
fn stroke_rejected() {
    assert_eq!(
        kind(&svg(r##"<rect width="1" height="1" stroke="#000"/>"##)),
        SvgErrorKind::Unsupported
    );
    // stroke="none" is a no-op and accepted
    assert!(import_svg(svg(r#"<rect width="1" height="1" stroke="none"/>"#).as_bytes()).is_ok());
}

#[test]
fn arc_rejected() {
    assert_eq!(kind(&svg(r#"<path d="M0 0 A5 5 0 0 1 10 10"/>"#)), SvgErrorKind::Unsupported);
}

#[test]
fn unsupported_elements_rejected() {
    for body in [
        r#"<filter id="f"/>"#,
        r#"<mask id="m"/>"#,
        r#"<clipPath id="c"/>"#,
        r#"<radialGradient id="r"/>"#,
        r#"<animate attributeName="x"/>"#,
        r#"<a href="https://example.com"/>"#,
        r#"<line x1="0" y1="0" x2="1" y2="1"/>"#,
    ] {
        let k = kind(&svg(body));
        assert!(
            matches!(k, SvgErrorKind::Unsupported | SvgErrorKind::ExternalReference),
            "{body}: {k:?}"
        );
    }
}

#[test]
fn group_opacity_rejected() {
    assert_eq!(
        kind(&svg(r#"<g opacity="0.5"><rect width="1" height="1"/></g>"#)),
        SvgErrorKind::Unsupported
    );
}

#[test]
fn foreign_namespace_rejected() {
    let s = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:i="http://example.com/ink" viewBox="0 0 1 1"><i:meta/></svg>"#;
    assert_eq!(kind(s), SvgErrorKind::Unsupported);
    let s = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:i="http://example.com/ink" viewBox="0 0 1 1"><rect i:x="1" width="1" height="1"/></svg>"#;
    assert_eq!(kind(s), SvgErrorKind::Unsupported);
}

#[test]
fn units_and_unknown_colors_rejected() {
    let s = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10mm" height="10mm"/>"#;
    assert_eq!(kind(s), SvgErrorKind::Unsupported);
    assert_eq!(
        kind(&svg(r#"<rect width="1" height="1" fill="currentColor"/>"#)),
        SvgErrorKind::Unsupported
    );
    assert_eq!(
        kind(&svg(r#"<rect width="1" height="1" fill="rgb(1,2,3)"/>"#)),
        SvgErrorKind::Unsupported
    );
}

#[test]
fn depth_limit_enforced() {
    let body = "<g>".repeat(MAX_DEPTH + 1) + &"</g>".repeat(MAX_DEPTH + 1);
    assert_eq!(kind(&svg(&body)), SvgErrorKind::ResourceLimit);
}

#[test]
fn element_limit_enforced() {
    let body = r#"<rect width="1" height="1"/>"#.repeat(MAX_ELEMENTS + 1);
    let k = kind(&svg(&body));
    assert_eq!(k, SvgErrorKind::ResourceLimit);
}

#[test]
fn huge_numbers_rejected() {
    assert_eq!(kind(&svg(r#"<rect width="1e300" height="1"/>"#)), SvgErrorKind::ResourceLimit);
}

#[test]
fn size_limit_enforced() {
    let big = vec![b' '; MAX_SVG_BYTES + 1];
    assert_eq!(import_svg(&big).unwrap_err().kind, SvgErrorKind::ResourceLimit);
}

#[test]
fn malformed_input_rejected() {
    assert_eq!(kind("<svg"), SvgErrorKind::Invalid);
    assert_eq!(kind(r#"<html xmlns="http://www.w3.org/1999/xhtml"/>"#), SvgErrorKind::Invalid);
    assert_eq!(kind(&svg(r#"<path d="L 1 1"/>"#)), SvgErrorKind::Invalid);
    assert_eq!(
        kind(&svg(r#"<rect width="1" height="1" fill="url(#missing)"/>"#)),
        SvgErrorKind::Invalid
    );
}
