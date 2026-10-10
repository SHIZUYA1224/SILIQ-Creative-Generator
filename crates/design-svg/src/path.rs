//! Path data (`d`) and number-list parsing. Arcs are rejected.

use crate::{MAX_ABS_NUMBER, Point, Segment, SvgError};

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn skip_separators(&mut self) {
        while self.i < self.s.len() && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',') {
            self.i += 1;
        }
    }

    fn at_end(&mut self) -> bool {
        self.skip_separators();
        self.i >= self.s.len()
    }

    fn peek_command(&mut self) -> Option<u8> {
        self.skip_separators();
        self.s
            .get(self.i)
            .copied()
            .filter(|c| c.is_ascii_alphabetic() && *c != b'e' && *c != b'E')
    }

    fn number(&mut self) -> Result<f64, SvgError> {
        self.skip_separators();
        let start = self.i;
        let s = self.s;
        let mut j = self.i;
        if j < s.len() && (s[j] == b'+' || s[j] == b'-') {
            j += 1;
        }
        let mut digits = 0;
        while j < s.len() && s[j].is_ascii_digit() {
            j += 1;
            digits += 1;
        }
        if j < s.len() && s[j] == b'.' {
            j += 1;
            while j < s.len() && s[j].is_ascii_digit() {
                j += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return Err(SvgError::invalid(format!("expected a number at byte {start}")));
        }
        if j < s.len() && (s[j] == b'e' || s[j] == b'E') {
            let mut k = j + 1;
            if k < s.len() && (s[k] == b'+' || s[k] == b'-') {
                k += 1;
            }
            let exp_start = k;
            while k < s.len() && s[k].is_ascii_digit() {
                k += 1;
            }
            if k > exp_start {
                j = k;
            }
        }
        self.i = j;
        let text = std::str::from_utf8(&s[start..j]).map_err(|_| SvgError::invalid("non-UTF-8 number"))?;
        let v: f64 = text.parse().map_err(|_| SvgError::invalid(format!("bad number {text:?}")))?;
        if !v.is_finite() || v.abs() > MAX_ABS_NUMBER {
            return Err(SvgError::new(
                crate::SvgErrorKind::ResourceLimit,
                format!("number {text} out of range"),
            ));
        }
        Ok(v)
    }
}

pub(crate) fn parse_number_list(s: &str) -> Result<Vec<f64>, SvgError> {
    let mut lx = Lexer { s: s.as_bytes(), i: 0 };
    let mut out = Vec::new();
    while !lx.at_end() {
        out.push(lx.number()?);
        if out.len() > crate::MAX_SEGMENTS {
            return Err(SvgError::new(crate::SvgErrorKind::ResourceLimit, "too many numbers"));
        }
    }
    Ok(out)
}

/// Parse path data into absolute segments (no transform applied).
pub(crate) fn parse_path_data(d: &str, budget: &mut usize) -> Result<Vec<Segment>, SvgError> {
    let mut lx = Lexer { s: d.as_bytes(), i: 0 };
    let mut out = Vec::new();
    let mut cur = Point { x: 0.0, y: 0.0 };
    let mut start = cur;
    let mut last_cubic_ctrl: Option<Point> = None;
    let mut last_quad_ctrl: Option<Point> = None;
    let mut cmd: Option<u8> = None;

    while !lx.at_end() {
        if let Some(c) = lx.peek_command() {
            lx.i += 1;
            cmd = Some(c);
        } else if cmd.is_none() {
            return Err(SvgError::invalid("path data must start with a command"));
        }
        let c = cmd.unwrap();
        let rel = c.is_ascii_lowercase();
        let off = |p: Point, cur: Point| {
            if rel {
                Point {
                    x: p.x + cur.x,
                    y: p.y + cur.y,
                }
            } else {
                p
            }
        };
        let pt = |lx: &mut Lexer| -> Result<Point, SvgError> {
            Ok(Point {
                x: lx.number()?,
                y: lx.number()?,
            })
        };

        let mut cubic_ctrl = None;
        let mut quad_ctrl = None;
        match c.to_ascii_uppercase() {
            b'M' => {
                cur = off(pt(&mut lx)?, cur);
                start = cur;
                out.push(Segment::MoveTo(cur));
                // Subsequent pairs are implicit line-tos.
                cmd = Some(if rel { b'l' } else { b'L' });
            }
            b'L' => {
                cur = off(pt(&mut lx)?, cur);
                out.push(Segment::LineTo(cur));
            }
            b'H' => {
                let x = lx.number()?;
                cur = Point {
                    x: if rel { cur.x + x } else { x },
                    y: cur.y,
                };
                out.push(Segment::LineTo(cur));
            }
            b'V' => {
                let y = lx.number()?;
                cur = Point {
                    x: cur.x,
                    y: if rel { cur.y + y } else { y },
                };
                out.push(Segment::LineTo(cur));
            }
            b'C' => {
                let c1 = off(pt(&mut lx)?, cur);
                let c2 = off(pt(&mut lx)?, cur);
                let p = off(pt(&mut lx)?, cur);
                out.push(Segment::CubicTo(c1, c2, p));
                cubic_ctrl = Some(c2);
                cur = p;
            }
            b'S' => {
                let c1 = reflect(last_cubic_ctrl, cur);
                let c2 = off(pt(&mut lx)?, cur);
                let p = off(pt(&mut lx)?, cur);
                out.push(Segment::CubicTo(c1, c2, p));
                cubic_ctrl = Some(c2);
                cur = p;
            }
            b'Q' => {
                let c1 = off(pt(&mut lx)?, cur);
                let p = off(pt(&mut lx)?, cur);
                out.push(Segment::QuadTo(c1, p));
                quad_ctrl = Some(c1);
                cur = p;
            }
            b'T' => {
                let c1 = reflect(last_quad_ctrl, cur);
                let p = off(pt(&mut lx)?, cur);
                out.push(Segment::QuadTo(c1, p));
                quad_ctrl = Some(c1);
                cur = p;
            }
            b'Z' => {
                out.push(Segment::Close);
                cur = start;
                // Z takes no arguments; a following number is an error.
                cmd = None;
            }
            b'A' => {
                return Err(SvgError::unsupported(
                    "arc commands (A/a) are not supported; convert arcs to curves",
                ));
            }
            other => return Err(SvgError::invalid(format!("unknown path command {:?}", other as char))),
        }
        last_cubic_ctrl = cubic_ctrl;
        last_quad_ctrl = quad_ctrl;
        if *budget == 0 {
            return Err(SvgError::new(crate::SvgErrorKind::ResourceLimit, "too many path segments"));
        }
        *budget -= 1;
    }
    if !matches!(out.first(), Some(Segment::MoveTo(_))) {
        return Err(SvgError::invalid("path data must start with M/m"));
    }
    Ok(out)
}

fn reflect(ctrl: Option<Point>, cur: Point) -> Point {
    match ctrl {
        Some(c) => Point {
            x: 2.0 * cur.x - c.x,
            y: 2.0 * cur.y - c.y,
        },
        None => cur,
    }
}
