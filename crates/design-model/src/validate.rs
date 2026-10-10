//! Schema-range checks that serde cannot express, plus semantic validation
//! (references, tree shape, slot typing, font-size rules).

use crate::issue::{Code, Issue, Issues};
use crate::types::*;
use std::collections::{HashMap, HashSet};

pub const MAX_DOCUMENT_BYTES: usize = 20 * 1024 * 1024;
const MAX_DIM: f64 = 16384.0;
const MAX_COORD: f64 = 1_000_000.0;

/// Parse a document from untrusted bytes. Only syntax/shape errors are
/// reported here; call [`validate`] afterwards.
pub fn parse_document(bytes: &[u8]) -> Result<DesignDocument, Issue> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(Issue::error(
            Code::ResourceLimit,
            format!("document is {} bytes; limit is {MAX_DOCUMENT_BYTES}", bytes.len()),
            "Reduce the document size.",
        ));
    }
    serde_json::from_slice(bytes).map_err(|e| {
        Issue::error(
            Code::Schema,
            format!("document does not match the core schema: {e}"),
            "Fix the JSON so that it conforms to schemas/design.core.schema.json.",
        )
    })
}

pub fn is_valid_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

pub fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|c| c.is_ascii_hexdigit())
}

pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

struct V {
    issues: Issues,
}

impl V {
    fn err(&mut self, issue: Issue) {
        self.issues.push(issue);
    }

    fn schema(&mut self, msg: String) -> &mut Issue {
        self.issues.push(Issue::error(
            Code::Schema,
            msg,
            "Fix the value so that it conforms to schemas/design.core.schema.json.",
        ));
        self.issues.last_mut().unwrap()
    }

    fn invalid(&mut self, msg: String, remedy: &str) -> &mut Issue {
        self.issues.push(Issue::error(Code::DocumentInvalid, msg, remedy));
        self.issues.last_mut().unwrap()
    }

    fn id(&mut self, what: &str, id: &str) {
        if !is_valid_id(id) {
            self.schema(format!("{what} id {id:?} must match ^[a-z][a-z0-9-]{{0,63}}$"));
        }
    }

    fn range(&mut self, what: &str, v: f64, min: f64, max: f64, min_exclusive: bool) -> bool {
        let ok = v.is_finite() && if min_exclusive { v > min } else { v >= min } && v <= max;
        if !ok {
            let lo = if min_exclusive { "(" } else { "[" };
            self.schema(format!("{what} = {v} is outside {lo}{min}, {max}]"));
        }
        ok
    }

    fn text_len(&mut self, what: &str, s: &str, max: usize) {
        if s.chars().count() > max {
            self.schema(format!("{what} is longer than {max} characters"));
        }
    }

    fn color(&mut self, what: &str, c: &str) {
        if !is_hex_color(c) {
            self.schema(format!("{what} {c:?} must be #RRGGBB"));
        }
    }

    fn bounds(&mut self, what: &str, b: &Bounds) {
        self.range(&format!("{what}.x"), b.x, -MAX_COORD, MAX_COORD, false);
        self.range(&format!("{what}.y"), b.y, -MAX_COORD, MAX_COORD, false);
        self.range(&format!("{what}.width"), b.width, 0.0, MAX_DIM, true);
        self.range(&format!("{what}.height"), b.height, 0.0, MAX_DIM, true);
    }

    fn count(&mut self, what: &str, n: usize, min: usize, max: usize) {
        if n < min || n > max {
            self.schema(format!("{what} has {n} items; allowed {min}..={max}"));
        }
    }
}

/// Validate a parsed document. Returns every blocking issue found.
pub fn validate(doc: &DesignDocument) -> Issues {
    let mut v = V { issues: Vec::new() };

    if doc.format != FORMAT {
        v.schema(format!("format must be {FORMAT:?}"));
    }
    if !SUPPORTED_SCHEMA_VERSIONS.contains(&doc.schema_version.as_str()) {
        v.err(Issue::error(
            Code::Schema,
            format!(
                "schemaVersion {:?} is not supported (this engine reads {SUPPORTED_SCHEMA_VERSIONS:?})",
                doc.schema_version
            ),
            "Open the document with an engine that supports this schema version.",
        ));
    }
    if doc.color_space != "sRGB" {
        v.schema("colorSpace must be \"sRGB\"".into());
    }
    v.id("document", &doc.document_id);
    v.text_len("title", &doc.title, 200);
    v.range("baseArtboard.width", doc.base_artboard.width, 0.0, MAX_DIM, true);
    v.range("baseArtboard.height", doc.base_artboard.height, 0.0, MAX_DIM, true);
    v.id("baseArtboard.rootNodeId", &doc.base_artboard.root_node_id);
    v.count("nodes", doc.nodes.len(), 1, 2000);
    v.count("fonts", doc.fonts.len(), 0, 100);
    v.count("assets", doc.assets.len(), 0, 2000);
    v.count("slots", doc.slots.len(), 0, 500);
    v.count("variants", doc.variants.len(), 1, 64);

    // --- fonts & assets ---
    let mut fonts: HashMap<&str, &Font> = HashMap::new();
    for f in &doc.fonts {
        v.id("font", &f.id);
        v.text_len("font.nameHint", &f.name_hint, 200);
        if f.face_index > 255 {
            v.schema(format!("font {} faceIndex {} > 255", f.id, f.face_index)).location.font_id = Some(f.id.clone());
        }
        check_hash_state(&mut v, f.state, f.sha256.as_deref(), "font", &f.id);
        if fonts.insert(&f.id, f).is_some() {
            v.invalid(format!("duplicate font id {:?}", f.id), "Give every font a unique id.")
                .location
                .font_id = Some(f.id.clone());
        }
    }
    let mut assets: HashMap<&str, &Asset> = HashMap::new();
    for a in &doc.assets {
        v.id("asset", &a.id);
        check_hash_state(&mut v, a.state, a.sha256.as_deref(), "asset", &a.id);
        let vector_mime = a.mime_hint == MimeHint::Svg;
        if (a.kind == AssetKind::Vector) != vector_mime {
            v.invalid(
                format!("asset {:?}: kind and mimeHint disagree", a.id),
                "Use kind \"vector\" exactly for image/svg+xml assets.",
            )
            .location
            .asset_id = Some(a.id.clone());
        }
        if assets.insert(&a.id, a).is_some() {
            v.invalid(format!("duplicate asset id {:?}", a.id), "Give every asset a unique id.")
                .location
                .asset_id = Some(a.id.clone());
        }
    }

    // --- nodes ---
    let mut nodes: HashMap<&str, &Node> = HashMap::new();
    for n in &doc.nodes {
        let id = n.id();
        v.id("node", id);
        if nodes.insert(id, n).is_some() {
            v.invalid(format!("duplicate node id {id:?}"), "Give every node a unique id.")
                .location
                .node_id = Some(id.to_owned());
        }
    }
    for n in &doc.nodes {
        check_node(&mut v, n, &fonts, &assets, doc.schema_version != "0.2.0-core");
    }

    // --- tree ---
    check_tree(&mut v, doc, &nodes);

    // --- slots ---
    let mut slot_ids = HashSet::new();
    for s in &doc.slots {
        v.id("slot", &s.id);
        v.id("slot.nodeId", &s.node_id);
        if !slot_ids.insert(s.id.as_str()) {
            v.invalid(format!("duplicate slot id {:?}", s.id), "Give every slot a unique id.");
        }
        if let Some(m) = s.max_length
            && !(1..=10000).contains(&m)
        {
            v.schema(format!("slot {} maxLength {m} outside 1..=10000", s.id));
        }
        let ok = match (s.slot_type, s.target, nodes.get(s.node_id.as_str())) {
            (SlotType::Text, SlotTarget::TextValue, Some(Node::Text { .. })) => true,
            (SlotType::Image, SlotTarget::AssetId, Some(Node::Image { .. })) => true,
            (_, _, None) => {
                v.invalid(
                    format!("slot {:?} refers to missing node {:?}", s.id, s.node_id),
                    "Point the slot at an existing node.",
                );
                continue;
            }
            _ => false,
        };
        if !ok {
            v.invalid(
                format!(
                    "slot {:?} ({:?} → {:?}) does not match node type {}",
                    s.id,
                    s.slot_type,
                    s.target,
                    nodes[s.node_id.as_str()].type_name()
                ),
                "text slots target text.value of a text node; image slots target assetId of an image node.",
            )
            .location
            .node_id = Some(s.node_id.clone());
        }
    }

    // --- variants ---
    let mut variant_ids = HashSet::new();
    for var in &doc.variants {
        v.id("variant", &var.id);
        v.text_len("variant.label", &var.label, 100);
        if !variant_ids.insert(var.id.as_str()) {
            v.invalid(format!("duplicate variant id {:?}", var.id), "Give every variant a unique id.");
        }
        v.range(&format!("variant {} width", var.id), var.width, 0.0, MAX_DIM, true);
        v.range(&format!("variant {} height", var.id), var.height, 0.0, MAX_DIM, true);
        v.count(&format!("variant {} nodePatches", var.id), var.node_patches.len(), 0, 2000);
        let mut patched = HashSet::new();
        for p in &var.node_patches {
            v.id("nodePatch.nodeId", &p.node_id);
            v.bounds(&format!("variant {} patch {}", var.id, p.node_id), &p.bounds);
            if !patched.insert(p.node_id.as_str()) {
                v.invalid(
                    format!("variant {:?} patches node {:?} twice", var.id, p.node_id),
                    "Merge the patches into one.",
                )
                .location
                .variant_id = Some(var.id.clone());
            }
            match (nodes.get(p.node_id.as_str()), p.font_size) {
                (None, _) => {
                    let i = v.invalid(
                        format!("variant {:?} patches missing node {:?}", var.id, p.node_id),
                        "Patch only existing nodes.",
                    );
                    i.location.variant_id = Some(var.id.clone());
                }
                (Some(Node::Text { text, .. }), Some(fs)) => {
                    v.range(&format!("variant {} patch {} fontSize", var.id, p.node_id), fs, 0.0, 2000.0, true);
                    check_font_size_allowed(&mut v, &p.node_id, text, fs, Some(&var.id));
                }
                (Some(n), Some(_)) => {
                    let i = v.invalid(
                        format!("variant {:?} sets fontSize on non-text node {:?}", var.id, n.id()),
                        "fontSize patches apply only to text nodes.",
                    );
                    i.location.variant_id = Some(var.id.clone());
                    i.location.node_id = Some(n.id().to_owned());
                }
                _ => {}
            }
            if let Some(lh) = p.line_height {
                v.range(&format!("variant {} patch {} lineHeight", var.id, p.node_id), lh, 0.0, 4000.0, true);
                let is_text = matches!(nodes.get(p.node_id.as_str()), Some(Node::Text { .. }));
                if doc.schema_version == "0.2.0-core" || !is_text {
                    let i = v.schema(format!(
                        "variant {:?} patch {:?}: lineHeight needs schemaVersion 0.3.0-core and a text node",
                        var.id, p.node_id
                    ));
                    i.location.variant_id = Some(var.id.clone());
                }
            }
        }
    }

    v.issues
}

fn check_hash_state(v: &mut V, state: ResolveState, sha: Option<&str>, what: &str, id: &str) {
    let ok = match (state, sha) {
        (ResolveState::Resolved, Some(h)) => is_sha256_hex(h),
        (ResolveState::Unresolved, None) => true,
        _ => false,
    };
    if !ok {
        let i = v.schema(format!(
            "{what} {id:?}: resolved entries need a lowercase sha256; unresolved entries need null"
        ));
        if what == "font" {
            i.location.font_id = Some(id.to_owned());
        } else {
            i.location.asset_id = Some(id.to_owned());
        }
    }
}

fn check_node(v: &mut V, n: &Node, fonts: &HashMap<&str, &Font>, assets: &HashMap<&str, &Asset>, effects_allowed: bool) {
    let id = n.id();
    let (bounds, rot, opacity, _) = n.common();
    let start = v.issues.len();
    v.bounds(&format!("node {id} bounds"), bounds);
    v.range(&format!("node {id} rotationDeg"), rot, -360.0, 360.0, false);
    v.range(&format!("node {id} opacity"), opacity, 0.0, 1.0, false);
    match n {
        Node::Group { name, child_ids, .. } => {
            v.text_len("node.name", name, 200);
            v.count(&format!("node {id} childIds"), child_ids.len(), 0, 2000);
        }
        Node::Rect {
            name, fill, corner_radius, ..
        } => {
            v.text_len("node.name", name, 200);
            v.range(&format!("node {id} cornerRadius"), *corner_radius, 0.0, 8192.0, false);
            check_paint(v, id, fill);
        }
        Node::Text {
            name,
            text,
            fill,
            outline,
            shadow,
            ..
        } => {
            v.text_len("node.name", name, 200);
            v.color(&format!("node {id} fill.color"), &fill.color);
            if (outline.is_some() || shadow.is_some()) && !effects_allowed {
                v.schema(format!("node {id}: outline / shadow need schemaVersion 0.3.0-core"));
            }
            if let Some(o) = outline {
                v.color(&format!("node {id} outline.color"), &o.color);
                v.range(&format!("node {id} outline.width"), o.width, 0.0, 200.0, true);
            }
            if let Some(sh) = shadow {
                v.color(&format!("node {id} shadow.color"), &sh.color);
                v.range(&format!("node {id} shadow.opacity"), sh.opacity, 0.0, 1.0, false);
                v.range(&format!("node {id} shadow.offsetX"), sh.offset_x, -1000.0, 1000.0, false);
                v.range(&format!("node {id} shadow.offsetY"), sh.offset_y, -1000.0, 1000.0, false);
                v.range(&format!("node {id} shadow.blur"), sh.blur, 0.0, 200.0, false);
            }
            v.text_len(&format!("node {id} text.value"), &text.value, 10000);
            v.range(&format!("node {id} fontSize"), text.font_size, 0.0, 2000.0, true);
            v.range(&format!("node {id} lineHeight"), text.line_height, 0.0, 4000.0, true);
            v.range(&format!("node {id} letterSpacing"), text.letter_spacing, -100.0, 100.0, false);
            if !(1..=100).contains(&text.max_lines) {
                v.schema(format!("node {id} maxLines {} outside 1..=100", text.max_lines));
            }
            v.count(&format!("node {id} allowedFontSizes"), text.allowed_font_sizes.len(), 0, 200);
            for s in &text.allowed_font_sizes {
                v.range(&format!("node {id} allowedFontSizes item"), *s, 0.0, 2000.0, true);
            }
            v.id("text.fontId", &text.font_id);
            if !fonts.contains_key(text.font_id.as_str()) {
                v.err(
                    Issue::error(
                        Code::FontMissing,
                        format!("text refers to font id {:?} that is not declared", text.font_id),
                        "Declare the font in fonts[] or choose a declared font.",
                    )
                    .node(id)
                    .font(&text.font_id),
                );
            }
            check_fit_rules(v, id, text);
        }
        Node::Image {
            name,
            asset_id,
            focal_point,
            clip_to_bounds,
            ..
        } => {
            v.text_len("node.name", name, 200);
            v.range(&format!("node {id} focalPoint.x"), focal_point.x, 0.0, 1.0, false);
            v.range(&format!("node {id} focalPoint.y"), focal_point.y, 0.0, 1.0, false);
            if !clip_to_bounds {
                v.schema(format!("node {id} clipToBounds must be true"));
            }
            match assets.get(asset_id.as_str()) {
                Some(a) if a.kind == AssetKind::Raster => {}
                Some(_) => {
                    v.err(
                        Issue::error(
                            Code::AssetType,
                            "image nodes accept raster assets only",
                            "Use a PNG/JPEG asset, or a logo node for vector assets.",
                        )
                        .node(id)
                        .asset(asset_id),
                    );
                }
                None => v.err(
                    Issue::error(
                        Code::AssetMissing,
                        format!("asset id {asset_id:?} is not declared"),
                        "Declare the asset in assets[].",
                    )
                    .node(id)
                    .asset(asset_id),
                ),
            }
        }
        Node::Logo {
            name,
            asset_id,
            preserve_aspect_ratio,
            min_width,
            clear_space,
            ..
        } => {
            v.text_len("node.name", name, 200);
            if !preserve_aspect_ratio {
                v.schema(format!("node {id} preserveAspectRatio must be true"));
            }
            v.range(&format!("node {id} minWidth"), *min_width, 0.0, MAX_DIM, true);
            v.range(&format!("node {id} clearSpace"), *clear_space, 0.0, 8192.0, false);
            if !assets.contains_key(asset_id.as_str()) {
                v.err(
                    Issue::error(
                        Code::AssetMissing,
                        format!("asset id {asset_id:?} is not declared"),
                        "Declare the asset in assets[].",
                    )
                    .node(id)
                    .asset(asset_id),
                );
            }
        }
    }
    for i in &mut v.issues[start..] {
        if i.location.node_id.is_none() {
            i.location.node_id = Some(id.to_owned());
        }
    }
}

fn check_paint(v: &mut V, id: &str, p: &Paint) {
    match p {
        Paint::Solid { color } => v.color(&format!("node {id} fill.color"), color),
        Paint::LinearGradient { angle_deg, stops } => {
            v.range(&format!("node {id} angleDeg"), *angle_deg, -360.0, 360.0, false);
            v.count(&format!("node {id} gradient stops"), stops.len(), 2, 16);
            let mut prev = f64::NEG_INFINITY;
            for s in stops {
                v.range(&format!("node {id} stop offset"), s.offset, 0.0, 1.0, false);
                v.range(&format!("node {id} stop opacity"), s.opacity, 0.0, 1.0, false);
                v.color(&format!("node {id} stop color"), &s.color);
                if s.offset < prev {
                    v.invalid(
                        format!("node {id}: gradient stop offsets must not decrease"),
                        "Order the stops by offset.",
                    );
                }
                prev = s.offset;
            }
        }
    }
}

/// Fit rules (TXT-005): with `fit-approved-sizes`, the author lists the
/// candidate sizes in strictly descending order and the nominal fontSize must
/// be one of them. With `error`, no candidates may be listed.
fn check_fit_rules(v: &mut V, id: &str, t: &TextSpec) {
    match t.overflow_policy {
        OverflowPolicy::Error => {
            if !t.allowed_font_sizes.is_empty() {
                v.invalid(
                    format!("node {id}: overflowPolicy \"error\" must not list allowedFontSizes"),
                    "Use overflowPolicy \"fit-approved-sizes\" or remove allowedFontSizes.",
                );
            }
        }
        OverflowPolicy::FitApprovedSizes => {
            if t.allowed_font_sizes.is_empty() {
                v.invalid(
                    format!("node {id}: fit-approved-sizes needs at least one allowed size"),
                    "List the approved sizes in descending order.",
                );
            }
            if t.allowed_font_sizes.windows(2).any(|w| w[1] >= w[0]) {
                v.invalid(
                    format!("node {id}: allowedFontSizes must be strictly descending"),
                    "Sort the approved sizes from largest to smallest without duplicates.",
                );
            }
            check_font_size_allowed(v, id, t, t.font_size, None);
        }
    }
}

fn check_font_size_allowed(v: &mut V, id: &str, t: &TextSpec, size: f64, variant: Option<&str>) {
    if t.overflow_policy == OverflowPolicy::FitApprovedSizes && !t.allowed_font_sizes.contains(&size) {
        let i = v.invalid(
            format!("node {id}: fontSize {size} is not in allowedFontSizes"),
            "Use one of the approved sizes.",
        );
        i.location.node_id = Some(id.to_owned());
        i.location.variant_id = variant.map(str::to_owned);
    }
}

fn check_tree(v: &mut V, doc: &DesignDocument, nodes: &HashMap<&str, &Node>) {
    let root = doc.base_artboard.root_node_id.as_str();
    match nodes.get(root) {
        None => {
            v.invalid(format!("root node {root:?} does not exist"), "Point rootNodeId at a group node.");
            return;
        }
        Some(Node::Group { .. }) => {}
        Some(_) => {
            v.invalid(format!("root node {root:?} is not a group"), "Use a group as the artboard root.");
            return;
        }
    }
    let mut parent: HashMap<&str, &str> = HashMap::new();
    for n in &doc.nodes {
        if let Node::Group { id, child_ids, .. } = n {
            for c in child_ids {
                if !nodes.contains_key(c.as_str()) {
                    v.invalid(
                        format!("group {id:?} lists missing child {c:?}"),
                        "Remove the reference or add the node.",
                    )
                    .location
                    .node_id = Some(id.clone());
                    continue;
                }
                if c == root {
                    v.invalid(
                        format!("root {root:?} appears as a child of {id:?}"),
                        "The root cannot have a parent.",
                    )
                    .location
                    .node_id = Some(id.clone());
                }
                if let Some(prev) = parent.insert(c.as_str(), id.as_str()) {
                    v.invalid(
                        format!("node {c:?} has two parents ({prev:?} and {id:?})"),
                        "Each node must appear in exactly one childIds list.",
                    )
                    .location
                    .node_id = Some(c.clone());
                }
            }
        }
    }
    // Cycle detection: walk each node up to the root.
    for n in &doc.nodes {
        let mut seen = HashSet::new();
        let mut cur = n.id();
        while let Some(p) = parent.get(cur) {
            if !seen.insert(cur) {
                v.invalid(format!("cycle in node tree at {cur:?}"), "Remove the cyclic childIds reference.")
                    .location
                    .node_id = Some(cur.to_owned());
                break;
            }
            cur = p;
        }
    }
    // Reachability from the root.
    let mut reached = HashSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !reached.insert(id) {
            continue;
        }
        if let Some(Node::Group { child_ids, .. }) = nodes.get(id) {
            stack.extend(child_ids.iter().map(String::as_str).filter(|c| nodes.contains_key(c)));
        }
    }
    for n in &doc.nodes {
        if !reached.contains(n.id()) {
            v.invalid(
                format!("node {:?} is not reachable from the root", n.id()),
                "Attach it to the tree or delete it.",
            )
            .location
            .node_id = Some(n.id().to_owned());
        }
    }
}
