//! Output file names (BAT-005): plain names inside the granted directory.

use design_model::{Code, Issue};

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4",
    "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Accept `name` only if it is a plain file name made of `A-Z a-z 0-9 . _ -`,
/// starting with a letter or digit, without `..`, ending in `.ext`, and not a
/// Windows device name.
pub fn validate_output_name(name: &str, ext: &str) -> Result<(), Issue> {
    let deny = |why: &str| {
        Issue::error(
            Code::PathDenied,
            format!("output name {name:?} rejected: {why}"),
            "Use a plain file name made of A-Z a-z 0-9 . _ - with the right extension.",
        )
    };
    if name.len() > 128 {
        return Err(deny("longer than 128 bytes"));
    }
    let b = name.as_bytes();
    if b.is_empty() || !b[0].is_ascii_alphanumeric() {
        return Err(deny("must start with a letter or digit"));
    }
    if !b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-')) {
        return Err(deny("contains a character outside A-Z a-z 0-9 . _ -"));
    }
    if name.contains("..") {
        return Err(deny("contains '..'"));
    }
    let Some(stem) = name.strip_suffix(&format!(".{ext}")) else {
        return Err(deny(&format!("must end with .{ext}")));
    };
    let base = stem.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&base.as_str()) {
        return Err(deny("reserved device name"));
    }
    Ok(())
}

/// Expand a name pattern. Only `{rowId}` and `{variantId}` are recognised;
/// any other brace is an error (no expressions are evaluated).
pub fn expand_pattern(pattern: &str, row_id: &str, variant_id: &str) -> Result<String, Issue> {
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let close = rest[open..].find('}').map(|c| open + c).ok_or_else(|| bad_pattern(pattern))?;
        match &rest[open + 1..close] {
            "rowId" => out.push_str(row_id),
            "variantId" => out.push_str(variant_id),
            _ => return Err(bad_pattern(pattern)),
        }
        rest = &rest[close + 1..];
    }
    if rest.contains('}') {
        return Err(bad_pattern(pattern));
    }
    out.push_str(rest);
    Ok(out)
}

fn bad_pattern(pattern: &str) -> Issue {
    Issue::error(
        Code::PathDenied,
        format!("name pattern {pattern:?} is invalid"),
        "Use only the placeholders {rowId} and {variantId}.",
    )
}
