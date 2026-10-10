use crate::{Point, SvgError};

/// 2D affine matrix `[a c e; b d f; 0 0 1]` (SVG `matrix(a b c d e f)` order).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// `self ∘ other`: apply `other` first, then `self`.
    pub fn then_after(&self, other: &Affine) -> Affine {
        Affine {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            e: self.a * other.e + self.c * other.f + self.e,
            f: self.b * other.e + self.d * other.f + self.f,
        }
    }

    pub fn apply(&self, p: Point) -> Point {
        Point {
            x: self.a * p.x + self.c * p.y + self.e,
            y: self.b * p.x + self.d * p.y + self.f,
        }
    }

    fn translate(x: f64, y: f64) -> Affine {
        Affine {
            e: x,
            f: y,
            ..Affine::IDENTITY
        }
    }
}

/// Parse an SVG `transform` / `gradientTransform` list.
pub(crate) fn parse_transform(s: &str) -> Result<Affine, SvgError> {
    let mut m = Affine::IDENTITY;
    let mut rest = s.trim();
    while !rest.is_empty() {
        let open = rest.find('(').ok_or_else(|| SvgError::invalid(format!("bad transform {s:?}")))?;
        let close = rest.find(')').ok_or_else(|| SvgError::invalid(format!("bad transform {s:?}")))?;
        if close < open {
            return Err(SvgError::invalid(format!("bad transform {s:?}")));
        }
        let name = rest[..open].trim();
        let args = crate::path::parse_number_list(&rest[open + 1..close])?;
        let t = match (name, args.as_slice()) {
            ("matrix", [a, b, c, d, e, f]) => Affine {
                a: *a,
                b: *b,
                c: *c,
                d: *d,
                e: *e,
                f: *f,
            },
            ("translate", [x]) => Affine::translate(*x, 0.0),
            ("translate", [x, y]) => Affine::translate(*x, *y),
            ("scale", [x]) => Affine {
                a: *x,
                d: *x,
                ..Affine::IDENTITY
            },
            ("scale", [x, y]) => Affine {
                a: *x,
                d: *y,
                ..Affine::IDENTITY
            },
            ("rotate", [deg]) => rotation(*deg),
            ("rotate", [deg, cx, cy]) => Affine::translate(*cx, *cy)
                .then_after(&rotation(*deg))
                .then_after(&Affine::translate(-cx, -cy)),
            ("skewX", [deg]) => Affine {
                c: deg.to_radians().tan(),
                ..Affine::IDENTITY
            },
            ("skewY", [deg]) => Affine {
                b: deg.to_radians().tan(),
                ..Affine::IDENTITY
            },
            _ => return Err(SvgError::unsupported(format!("transform {name}({}) is not supported", args.len()))),
        };
        m = m.then_after(&t);
        rest = rest[close + 1..].trim_start_matches(|c: char| c.is_whitespace() || c == ',');
    }
    if [m.a, m.b, m.c, m.d, m.e, m.f].iter().any(|v| !v.is_finite()) {
        return Err(SvgError::invalid("transform is not finite"));
    }
    Ok(m)
}

fn rotation(deg: f64) -> Affine {
    let (s, c) = deg.to_radians().sin_cos();
    Affine {
        a: c,
        b: s,
        c: -s,
        d: c,
        e: 0.0,
        f: 0.0,
    }
}
