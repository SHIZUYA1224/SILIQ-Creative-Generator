use crate::path::{parse_number_list, parse_path_data};
use crate::transform::{Affine, parse_transform};
use crate::*;
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::HashMap;

const SVG_NS: &str = "http://www.w3.org/2000/svg";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

/// Bezier circle approximation constant.
const KAPPA: f64 = 0.552_284_749_830_793_4;

#[derive(Clone, Copy)]
struct Style {
    fill: FillSpec,
    fill_opacity: f64,
    fill_rule: FillRule,
}

#[derive(Clone, Copy, PartialEq)]
enum FillSpec {
    None,
    Rgb([u8; 3]),
    /// Index into the gradient table.
    Gradient(usize),
}

struct LinearDef {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    bbox_units: bool,
    transform: Affine,
    stops: Vec<GradientStop>,
}

struct Ctx {
    gradients: Vec<LinearDef>,
    gradient_ids: HashMap<String, usize>,
    shapes: Vec<Shape>,
    segment_budget: usize,
}

pub(crate) fn import(bytes: &[u8]) -> Result<VectorArt, SvgError> {
    if bytes.len() > MAX_SVG_BYTES {
        return Err(SvgError::new(
            SvgErrorKind::ResourceLimit,
            format!("SVG is {} bytes; limit {MAX_SVG_BYTES}", bytes.len()),
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| SvgError::invalid("SVG is not UTF-8"))?;
    let opts = ParsingOptions {
        allow_dtd: false,
        nodes_limit: (MAX_ELEMENTS * 4) as u32,
        ..ParsingOptions::default()
    };
    let doc = Document::parse_with_options(text, opts).map_err(|e| match e {
        roxmltree::Error::DtdDetected => SvgError::unsupported("DTD / entity declarations are not allowed"),
        roxmltree::Error::NodesLimitReached => SvgError::new(SvgErrorKind::ResourceLimit, "too many XML nodes"),
        other => SvgError::invalid(format!("XML parse error: {other}")),
    })?;

    let root = doc.root_element();
    if root.tag_name().name() != "svg" || root.tag_name().namespace() != Some(SVG_NS) {
        return Err(SvgError::invalid("root element must be <svg> in the SVG namespace"));
    }

    let elements = doc.descendants().filter(|n| n.is_element()).count();
    if elements > MAX_ELEMENTS {
        return Err(SvgError::new(
            SvgErrorKind::ResourceLimit,
            format!("{elements} elements; limit {MAX_ELEMENTS}"),
        ));
    }
    for n in doc.descendants() {
        if n.is_pi() {
            return Err(SvgError::unsupported(
                "processing instructions (e.g. xml-stylesheet) are not allowed",
            ));
        }
    }

    // Report external references first: they are a security finding, not
    // merely an unsupported feature.
    for n in doc.descendants().filter(|n| n.is_element()) {
        for a in n.attributes() {
            let v = a.value().trim();
            let external_href = a.name() == "href" && !v.starts_with('#');
            let external_url = v.contains("url(") && !v.starts_with("url(#");
            if external_href || external_url {
                return Err(SvgError::new(
                    SvgErrorKind::ExternalReference,
                    format!("external reference in <{}> {}", n.tag_name().name(), a.name()),
                ));
            }
        }
    }

    let view_box = parse_root(root)?;
    let mut ctx = Ctx {
        gradients: Vec::new(),
        gradient_ids: HashMap::new(),
        shapes: Vec::new(),
        segment_budget: MAX_SEGMENTS,
    };

    // Gradients may be referenced before they are defined, so collect them first.
    for n in root
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "linearGradient")
    {
        let parent_ok = n.parent_element().map(|p| p.tag_name().name() == "defs").unwrap_or(false);
        if !parent_ok {
            return Err(SvgError::unsupported("linearGradient must be a direct child of <defs>"));
        }
        let id = n.attribute("id").ok_or_else(|| SvgError::invalid("linearGradient without id"))?;
        let def = parse_linear_gradient(n)?;
        if ctx.gradient_ids.insert(id.to_owned(), ctx.gradients.len()).is_some() {
            return Err(SvgError::invalid(format!("duplicate id {id:?}")));
        }
        ctx.gradients.push(def);
    }

    let style = Style {
        fill: FillSpec::Rgb([0, 0, 0]),
        fill_opacity: 1.0,
        fill_rule: FillRule::NonZero,
    };
    walk_children(root, &mut ctx, Affine::IDENTITY, style, 1)?;

    Ok(VectorArt {
        view_box,
        shapes: ctx.shapes,
    })
}

fn parse_root(root: Node) -> Result<ViewBox, SvgError> {
    check_attrs(root, &["viewBox", "width", "height", "version", "preserveAspectRatio", "id"])?;
    if let Some(par) = root.attribute("preserveAspectRatio")
        && par.trim() != "xMidYMid meet"
        && par.trim() != "xMidYMid"
    {
        return Err(SvgError::unsupported(format!("preserveAspectRatio {par:?} is not supported")));
    }
    if let Some(vb) = root.attribute("viewBox") {
        let v = parse_number_list(vb)?;
        if let [x, y, w, h] = v[..]
            && w > 0.0
            && h > 0.0
        {
            return Ok(ViewBox { x, y, width: w, height: h });
        }
        return Err(SvgError::invalid(format!("invalid viewBox {vb:?}")));
    }
    let w = root.attribute("width").map(plain_length).transpose()?;
    let h = root.attribute("height").map(plain_length).transpose()?;
    match (w, h) {
        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Ok(ViewBox {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
        }),
        _ => Err(SvgError::invalid("SVG needs a viewBox or positive width and height")),
    }
}

/// A length without units (or `px`). Other units would need a DPI assumption.
fn plain_length(s: &str) -> Result<f64, SvgError> {
    let t = s.trim().strip_suffix("px").unwrap_or(s.trim());
    match parse_number_list(t).as_deref() {
        Ok([v]) => Ok(*v),
        _ => Err(SvgError::unsupported(format!("length {s:?} is not supported (use unitless or px)"))),
    }
}

fn check_attrs(n: Node, allowed: &[&str]) -> Result<(), SvgError> {
    for a in n.attributes() {
        let name = a.name();
        let ns = a.namespace();
        if name == "href" {
            let v = a.value().trim();
            return Err(if v.starts_with('#') {
                SvgError::unsupported(format!("<{}> href references are not supported", n.tag_name().name()))
            } else {
                SvgError::new(
                    SvgErrorKind::ExternalReference,
                    format!("external reference in <{}> href", n.tag_name().name()),
                )
            });
        }
        if a.value().contains("url(") && !a.value().trim().starts_with("url(#") {
            return Err(SvgError::new(
                SvgErrorKind::ExternalReference,
                format!("external url() in attribute {name}"),
            ));
        }
        if ns.is_some() && ns != Some(XLINK_NS) {
            return Err(SvgError::unsupported(format!("namespaced attribute {name:?} is not supported")));
        }
        if name.starts_with("on") {
            return Err(SvgError::unsupported(format!("event handler attribute {name:?} is not allowed")));
        }
        if !allowed.contains(&name) {
            return Err(SvgError::unsupported(format!(
                "attribute {name:?} on <{}> is not supported",
                n.tag_name().name()
            )));
        }
    }
    Ok(())
}

const SHAPE_ATTRS: &[&str] = &["id", "fill", "fill-opacity", "fill-rule", "opacity", "transform", "stroke"];

fn walk_children(parent: Node, ctx: &mut Ctx, ctm: Affine, style: Style, depth: usize) -> Result<(), SvgError> {
    if depth > MAX_DEPTH {
        return Err(SvgError::new(
            SvgErrorKind::ResourceLimit,
            format!("nesting deeper than {MAX_DEPTH}"),
        ));
    }
    for n in parent.children() {
        if n.is_text() {
            if n.text().map(|t| !t.trim().is_empty()).unwrap_or(false) && !matches!(parent.tag_name().name(), "title" | "desc") {
                return Err(SvgError::unsupported("character data outside <title>/<desc> is not supported"));
            }
            continue;
        }
        if !n.is_element() {
            continue; // comments
        }
        if n.tag_name().namespace() != Some(SVG_NS) {
            return Err(SvgError::unsupported(format!(
                "element <{}> outside the SVG namespace is not supported",
                n.tag_name().name()
            )));
        }
        let name = n.tag_name().name();
        match name {
            "title" | "desc" => {
                check_attrs(n, &["id"])?;
                if n.children().any(|c| c.is_element()) {
                    return Err(SvgError::unsupported(format!("<{name}> must contain text only")));
                }
            }
            "defs" => {
                check_attrs(n, &["id"])?;
                for c in n.children().filter(|c| c.is_element()) {
                    if c.tag_name().name() != "linearGradient" {
                        return Err(SvgError::unsupported(format!(
                            "<{}> inside <defs> is not supported",
                            c.tag_name().name()
                        )));
                    }
                }
            }
            "g" => {
                check_attrs(n, &["id", "fill", "fill-opacity", "fill-rule", "opacity", "transform", "stroke"])?;
                if let Some(o) = n.attribute("opacity")
                    && parse_unit_number(o)? != 1.0
                {
                    return Err(SvgError::unsupported(
                        "group opacity is not supported (it needs a compositing layer)",
                    ));
                }
                let (ctm, style) = element_state(n, ctx, ctm, style)?;
                walk_children(n, ctx, ctm, style, depth + 1)?;
            }
            "path" | "rect" | "circle" | "ellipse" | "polygon" | "polyline" => {
                if n.children().any(|c| c.is_element()) {
                    return Err(SvgError::unsupported(format!("child elements of <{name}> are not supported")));
                }
                let extra: &[&str] = match name {
                    "path" => &["d"],
                    "rect" => &["x", "y", "width", "height", "rx", "ry"],
                    "circle" => &["cx", "cy", "r"],
                    "ellipse" => &["cx", "cy", "rx", "ry"],
                    _ => &["points"],
                };
                let allowed: Vec<&str> = SHAPE_ATTRS.iter().chain(extra).copied().collect();
                check_attrs(n, &allowed)?;
                let (ctm, style) = element_state(n, ctx, ctm, style)?;
                let local = shape_segments(n, &mut ctx.segment_budget)?;
                let opacity = n.attribute("opacity").map(parse_unit_number).transpose()?.unwrap_or(1.0);
                push_shape(ctx, local, ctm, style, opacity)?;
            }
            "linearGradient" => {} // handled up front; only valid inside <defs>
            other => {
                return Err(match other {
                    "use" | "image" | "a" => SvgError::unsupported(format!("<{other}> is not supported (references are not resolved)")),
                    "script" | "foreignObject" => SvgError::unsupported(format!("<{other}> is not allowed")),
                    "text" | "tspan" | "textPath" => SvgError::unsupported("SVG text is not supported; supply outlined artwork"),
                    _ => SvgError::unsupported(format!("element <{other}> is not supported")),
                });
            }
        }
    }
    Ok(())
}

/// Apply this element's transform and inherited fill properties.
fn element_state(n: Node, ctx: &Ctx, ctm: Affine, mut style: Style) -> Result<(Affine, Style), SvgError> {
    if let Some(s) = n.attribute("stroke")
        && s.trim() != "none"
    {
        return Err(SvgError::unsupported("strokes are not supported; outline the strokes first"));
    }
    let ctm = match n.attribute("transform") {
        Some(t) => ctm.then_after(&parse_transform(t)?),
        None => ctm,
    };
    if let Some(f) = n.attribute("fill") {
        style.fill = parse_fill(f, ctx)?;
    }
    if let Some(o) = n.attribute("fill-opacity") {
        style.fill_opacity = parse_unit_number(o)?;
    }
    if let Some(r) = n.attribute("fill-rule") {
        style.fill_rule = match r.trim() {
            "nonzero" => FillRule::NonZero,
            "evenodd" => FillRule::EvenOdd,
            other => return Err(SvgError::unsupported(format!("fill-rule {other:?}"))),
        };
    }
    Ok((ctm, style))
}

fn parse_unit_number(s: &str) -> Result<f64, SvgError> {
    match parse_number_list(s)?.as_slice() {
        [v] if (0.0..=1.0).contains(v) => Ok(*v),
        _ => Err(SvgError::unsupported(format!("opacity value {s:?} must be a number in 0..1"))),
    }
}

fn parse_fill(s: &str, ctx: &Ctx) -> Result<FillSpec, SvgError> {
    let s = s.trim();
    if s == "none" {
        return Ok(FillSpec::None);
    }
    if let Some(inner) = s.strip_prefix("url(").and_then(|r| r.strip_suffix(')')) {
        let id = inner
            .trim()
            .strip_prefix('#')
            .ok_or_else(|| SvgError::new(SvgErrorKind::ExternalReference, format!("external fill {s:?}")))?;
        return ctx
            .gradient_ids
            .get(id)
            .map(|i| FillSpec::Gradient(*i))
            .ok_or_else(|| SvgError::invalid(format!("fill references unknown gradient #{id}")));
    }
    parse_color(s).map(FillSpec::Rgb)
}

fn parse_color(s: &str) -> Result<[u8; 3], SvgError> {
    let s = s.trim();
    let hex = |h: &str| u8::from_str_radix(h, 16).map_err(|_| SvgError::unsupported(format!("color {s:?}")));
    match s {
        "black" => return Ok([0, 0, 0]),
        "white" => return Ok([255, 255, 255]),
        _ => {}
    }
    let Some(h) = s.strip_prefix('#') else {
        return Err(SvgError::unsupported(format!("color {s:?} is not supported (use #rgb or #rrggbb)")));
    };
    if !h.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(SvgError::unsupported(format!("color {s:?}")));
    }
    match h.len() {
        6 => Ok([hex(&h[0..2])?, hex(&h[2..4])?, hex(&h[4..6])?]),
        3 => {
            let d = |i: usize| hex(&h[i..i + 1]).map(|v| v * 17);
            Ok([d(0)?, d(1)?, d(2)?])
        }
        _ => Err(SvgError::unsupported(format!("color {s:?} is not supported (use #rgb or #rrggbb)"))),
    }
}

fn parse_linear_gradient(n: Node) -> Result<LinearDef, SvgError> {
    check_attrs(
        n,
        &["id", "x1", "y1", "x2", "y2", "gradientUnits", "gradientTransform", "spreadMethod"],
    )?;
    let bbox_units = match n.attribute("gradientUnits").map(str::trim) {
        None | Some("objectBoundingBox") => true,
        Some("userSpaceOnUse") => false,
        Some(o) => return Err(SvgError::unsupported(format!("gradientUnits {o:?}"))),
    };
    if let Some(sm) = n.attribute("spreadMethod")
        && sm.trim() != "pad"
    {
        return Err(SvgError::unsupported(format!("spreadMethod {sm:?} is not supported")));
    }
    let coord = |name: &str, default: f64| -> Result<f64, SvgError> {
        let Some(v) = n.attribute(name) else { return Ok(default) };
        let v = v.trim();
        if let Some(p) = v.strip_suffix('%') {
            if !bbox_units {
                return Err(SvgError::unsupported("percentages with userSpaceOnUse are not supported"));
            }
            return match parse_number_list(p)?.as_slice() {
                [x] => Ok(x / 100.0),
                _ => Err(SvgError::invalid(format!("bad gradient coordinate {v:?}"))),
            };
        }
        match parse_number_list(v)?.as_slice() {
            [x] => Ok(*x),
            _ => Err(SvgError::invalid(format!("bad gradient coordinate {v:?}"))),
        }
    };
    let transform = match n.attribute("gradientTransform") {
        Some(t) => parse_transform(t)?,
        None => Affine::IDENTITY,
    };
    let mut stops = Vec::new();
    for c in n.children() {
        if c.is_text() {
            if c.text().map(|t| !t.trim().is_empty()).unwrap_or(false) {
                return Err(SvgError::unsupported("text inside linearGradient"));
            }
            continue;
        }
        if !c.is_element() {
            continue;
        }
        if c.tag_name().name() != "stop" || c.tag_name().namespace() != Some(SVG_NS) {
            return Err(SvgError::unsupported(format!("<{}> inside linearGradient", c.tag_name().name())));
        }
        check_attrs(c, &["offset", "stop-color", "stop-opacity", "id"])?;
        let off = c.attribute("offset").unwrap_or("0").trim();
        let mut offset = match off.strip_suffix('%') {
            Some(p) => parse_number_list(p)?.first().copied().unwrap_or(0.0) / 100.0,
            None => parse_number_list(off)?.first().copied().unwrap_or(0.0),
        };
        offset = offset.clamp(0.0, 1.0);
        // SVG: an offset smaller than the previous one is raised to it.
        if let Some(prev) = stops.last().map(|s: &GradientStop| s.offset) {
            offset = offset.max(prev);
        }
        let rgb = parse_color(c.attribute("stop-color").unwrap_or("black"))?;
        let opacity = c.attribute("stop-opacity").map(parse_unit_number).transpose()?.unwrap_or(1.0);
        stops.push(GradientStop { offset, rgb, opacity });
    }
    if stops.len() < 2 {
        return Err(SvgError::unsupported("linearGradient needs at least two stops"));
    }
    Ok(LinearDef {
        x1: coord("x1", 0.0)?,
        y1: coord("y1", 0.0)?,
        x2: coord("x2", 1.0)?,
        y2: coord("y2", 0.0)?,
        bbox_units,
        transform,
        stops,
    })
}

fn num_attr(n: Node, name: &str, default: Option<f64>) -> Result<f64, SvgError> {
    match n.attribute(name) {
        Some(v) => match parse_number_list(v.trim().strip_suffix("px").unwrap_or(v.trim()))?.as_slice() {
            [x] => Ok(*x),
            _ => Err(SvgError::unsupported(format!("attribute {name}={v:?} must be a plain number"))),
        },
        None => default.ok_or_else(|| SvgError::invalid(format!("<{}> requires {name}", n.tag_name().name()))),
    }
}

fn shape_segments(n: Node, budget: &mut usize) -> Result<Vec<Segment>, SvgError> {
    let p = |x: f64, y: f64| Point { x, y };
    let segs = match n.tag_name().name() {
        "path" => parse_path_data(n.attribute("d").unwrap_or(""), budget)?,
        "rect" => {
            let (x, y) = (num_attr(n, "x", Some(0.0))?, num_attr(n, "y", Some(0.0))?);
            let (w, h) = (num_attr(n, "width", None)?, num_attr(n, "height", None)?);
            if w <= 0.0 || h <= 0.0 {
                return Err(SvgError::invalid("rect with non-positive size"));
            }
            let rx_attr = n.attribute("rx").map(|_| num_attr(n, "rx", None)).transpose()?;
            let ry_attr = n.attribute("ry").map(|_| num_attr(n, "ry", None)).transpose()?;
            let rx = rx_attr.or(ry_attr).unwrap_or(0.0).clamp(0.0, w / 2.0);
            let ry = ry_attr.or(rx_attr).unwrap_or(0.0).clamp(0.0, h / 2.0);
            if rx == 0.0 || ry == 0.0 {
                vec![
                    Segment::MoveTo(p(x, y)),
                    Segment::LineTo(p(x + w, y)),
                    Segment::LineTo(p(x + w, y + h)),
                    Segment::LineTo(p(x, y + h)),
                    Segment::Close,
                ]
            } else {
                let (kx, ky) = (rx * KAPPA, ry * KAPPA);
                let (r, b) = (x + w, y + h);
                vec![
                    Segment::MoveTo(p(x + rx, y)),
                    Segment::LineTo(p(r - rx, y)),
                    Segment::CubicTo(p(r - rx + kx, y), p(r, y + ry - ky), p(r, y + ry)),
                    Segment::LineTo(p(r, b - ry)),
                    Segment::CubicTo(p(r, b - ry + ky), p(r - rx + kx, b), p(r - rx, b)),
                    Segment::LineTo(p(x + rx, b)),
                    Segment::CubicTo(p(x + rx - kx, b), p(x, b - ry + ky), p(x, b - ry)),
                    Segment::LineTo(p(x, y + ry)),
                    Segment::CubicTo(p(x, y + ry - ky), p(x + rx - kx, y), p(x + rx, y)),
                    Segment::Close,
                ]
            }
        }
        "circle" | "ellipse" => {
            let (cx, cy) = (num_attr(n, "cx", Some(0.0))?, num_attr(n, "cy", Some(0.0))?);
            let (rx, ry) = if n.tag_name().name() == "circle" {
                let r = num_attr(n, "r", None)?;
                (r, r)
            } else {
                (num_attr(n, "rx", None)?, num_attr(n, "ry", None)?)
            };
            if rx <= 0.0 || ry <= 0.0 {
                return Err(SvgError::invalid("circle/ellipse with non-positive radius"));
            }
            let (kx, ky) = (rx * KAPPA, ry * KAPPA);
            vec![
                Segment::MoveTo(p(cx + rx, cy)),
                Segment::CubicTo(p(cx + rx, cy + ky), p(cx + kx, cy + ry), p(cx, cy + ry)),
                Segment::CubicTo(p(cx - kx, cy + ry), p(cx - rx, cy + ky), p(cx - rx, cy)),
                Segment::CubicTo(p(cx - rx, cy - ky), p(cx - kx, cy - ry), p(cx, cy - ry)),
                Segment::CubicTo(p(cx + kx, cy - ry), p(cx + rx, cy - ky), p(cx + rx, cy)),
                Segment::Close,
            ]
        }
        name @ ("polygon" | "polyline") => {
            let v = parse_number_list(n.attribute("points").unwrap_or(""))?;
            if v.len() < 4 || v.len() % 2 != 0 {
                return Err(SvgError::invalid(format!(
                    "<{name}> needs an even number of coordinates (≥ 2 points)"
                )));
            }
            let mut segs: Vec<Segment> = v
                .chunks(2)
                .enumerate()
                .map(|(i, c)| {
                    if i == 0 {
                        Segment::MoveTo(p(c[0], c[1]))
                    } else {
                        Segment::LineTo(p(c[0], c[1]))
                    }
                })
                .collect();
            // A filled polyline is filled as if closed (SVG semantics).
            segs.push(Segment::Close);
            segs
        }
        _ => unreachable!(),
    };
    if segs.len() > *budget {
        return Err(SvgError::new(SvgErrorKind::ResourceLimit, "too many path segments"));
    }
    *budget -= segs.len().min(*budget);
    Ok(segs)
}

fn push_shape(ctx: &mut Ctx, local: Vec<Segment>, ctm: Affine, style: Style, opacity: f64) -> Result<(), SvgError> {
    let fill = match style.fill {
        FillSpec::None => return Ok(()),
        FillSpec::Rgb(rgb) => Fill::Solid { rgb },
        FillSpec::Gradient(i) => {
            let g = &ctx.gradients[i];
            // Gradient space → element user space (before the element CTM).
            let to_user = if g.bbox_units {
                let (min, max) = local_bounds(&local).ok_or_else(|| SvgError::invalid("empty shape"))?;
                let (w, h) = (max.x - min.x, max.y - min.y);
                if w <= 0.0 || h <= 0.0 {
                    return Err(SvgError::unsupported("objectBoundingBox gradient on a zero-area shape"));
                }
                Affine {
                    a: w,
                    b: 0.0,
                    c: 0.0,
                    d: h,
                    e: min.x,
                    f: min.y,
                }
                .then_after(&g.transform)
            } else {
                g.transform
            };
            Fill::Linear {
                start: Point { x: g.x1, y: g.y1 },
                end: Point { x: g.x2, y: g.y2 },
                matrix: ctm.then_after(&to_user),
                stops: g.stops.clone(),
            }
        }
    };
    let segments = local
        .into_iter()
        .map(|s| match s {
            Segment::MoveTo(p) => Segment::MoveTo(ctm.apply(p)),
            Segment::LineTo(p) => Segment::LineTo(ctm.apply(p)),
            Segment::QuadTo(a, p) => Segment::QuadTo(ctm.apply(a), ctm.apply(p)),
            Segment::CubicTo(a, b, p) => Segment::CubicTo(ctm.apply(a), ctm.apply(b), ctm.apply(p)),
            Segment::Close => Segment::Close,
        })
        .collect();
    ctx.shapes.push(Shape {
        segments,
        fill,
        fill_rule: style.fill_rule,
        opacity: style.fill_opacity * opacity,
    });
    Ok(())
}

/// Exact bounds of the outline (curve extrema included), as SVG's
/// objectBoundingBox requires.
fn local_bounds(segs: &[Segment]) -> Option<(Point, Point)> {
    let mut pts: Vec<Point> = Vec::new();
    let mut cur: Option<Point> = None;
    let mut start: Option<Point> = None;
    for s in segs {
        match *s {
            Segment::MoveTo(p) => {
                pts.push(p);
                cur = Some(p);
                start = Some(p);
            }
            Segment::LineTo(p) => {
                pts.push(p);
                cur = Some(p);
            }
            Segment::QuadTo(c, p) => {
                let p0 = cur?;
                pts.push(p);
                for axis in 0..2 {
                    let (a, b, e) = (get(p0, axis), get(c, axis), get(p, axis));
                    let den = a - 2.0 * b + e;
                    if den != 0.0 {
                        let t = (a - b) / den;
                        if t > 0.0 && t < 1.0 {
                            pts.push(quad_at(p0, c, p, t));
                        }
                    }
                }
                cur = Some(p);
            }
            Segment::CubicTo(c1, c2, p) => {
                let p0 = cur?;
                pts.push(p);
                for axis in 0..2 {
                    let (a, b, c, d) = (get(p0, axis), get(c1, axis), get(c2, axis), get(p, axis));
                    // Derivative / 3: qa t² + qb t + qc
                    let qa = -a + 3.0 * b - 3.0 * c + d;
                    let qb = 2.0 * (a - 2.0 * b + c);
                    let qc = b - a;
                    for t in quadratic_roots(qa, qb, qc) {
                        if t > 0.0 && t < 1.0 {
                            pts.push(cubic_at(p0, c1, c2, p, t));
                        }
                    }
                }
                cur = Some(p);
            }
            Segment::Close => cur = start,
        }
    }
    let first = *pts.first()?;
    let (mut min, mut max) = (first, first);
    for p in pts {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    }
    Some((min, max))
}

fn get(p: Point, axis: usize) -> f64 {
    if axis == 0 { p.x } else { p.y }
}

fn quad_at(p0: Point, c: Point, p: Point, t: f64) -> Point {
    let u = 1.0 - t;
    Point {
        x: u * u * p0.x + 2.0 * u * t * c.x + t * t * p.x,
        y: u * u * p0.y + 2.0 * u * t * c.y + t * t * p.y,
    }
}

fn cubic_at(p0: Point, c1: Point, c2: Point, p: Point, t: f64) -> Point {
    let u = 1.0 - t;
    let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Point {
        x: w0 * p0.x + w1 * c1.x + w2 * c2.x + w3 * p.x,
        y: w0 * p0.y + w1 * c1.y + w2 * c2.y + w3 * p.y,
    }
}

fn quadratic_roots(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a.abs() < 1e-12 {
        return if b.abs() < 1e-12 { vec![] } else { vec![-c / b] };
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return vec![];
    }
    let r = disc.sqrt();
    vec![(-b + r) / (2.0 * a), (-b - r) / (2.0 * a)]
}
