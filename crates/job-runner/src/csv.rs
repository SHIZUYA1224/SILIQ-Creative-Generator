//! Strict CSV reader for batch data (BAT-001).
//!
//! RFC 4180 subset: comma separator, `"` quoting with `""` escapes, CRLF or
//! LF record ends, quoted fields may contain commas and line breaks. UTF-8
//! only (a leading BOM is removed). Every value is kept byte-for-byte: no
//! trimming, no number parsing (leading zeros survive), no formula
//! evaluation. Anything ambiguous is an error with its line number.

use design_model::{Code, Issue};

pub const MAX_CSV_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_ROWS: usize = 10_000;
pub const ROW_ID_COLUMN: &str = "rowId";

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub header: Vec<String>,
    /// (1-based line number where the record starts, fields)
    pub records: Vec<(usize, Vec<String>)>,
}

fn err(line: usize, msg: impl Into<String>) -> Issue {
    Issue::error(
        Code::DataInvalid,
        format!("CSV line {line}: {}", msg.into()),
        "Fix the CSV (UTF-8, comma-separated, RFC 4180 quoting).",
    )
}

pub fn parse(bytes: &[u8]) -> Result<Table, Issue> {
    if bytes.len() > MAX_CSV_BYTES {
        return Err(Issue::error(
            Code::ResourceLimit,
            format!("CSV is {} bytes; limit is {MAX_CSV_BYTES}", bytes.len()),
            "Split the data into smaller files.",
        ));
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|e| {
        Issue::error(
            Code::DataInvalid,
            format!("CSV is not valid UTF-8 (byte {})", e.valid_up_to()),
            "Save the CSV as UTF-8. Other encodings are not converted automatically.",
        )
    })?;

    let mut records: Vec<(usize, Vec<String>)> = Vec::new();
    let mut field = String::new();
    let mut fields: Vec<String> = Vec::new();
    let mut line = 1usize;
    let mut record_line = 1usize;
    let mut chars = text.chars().peekable();
    let mut at_field_start = true;
    let mut in_quotes = false;
    let mut after_quote = false; // just closed a quoted field

    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => {
                    in_quotes = false;
                    after_quote = true;
                }
                '\r' if chars.peek() == Some(&'\n') => {
                    chars.next();
                    field.push('\n');
                    line += 1;
                }
                '\r' => return Err(err(line, "bare carriage return")),
                '\n' => {
                    field.push('\n');
                    line += 1;
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            ',' => {
                fields.push(std::mem::take(&mut field));
                at_field_start = true;
                after_quote = false;
            }
            '\r' | '\n' => {
                if c == '\r' {
                    if chars.peek() != Some(&'\n') {
                        return Err(err(line, "bare carriage return"));
                    }
                    chars.next();
                }
                fields.push(std::mem::take(&mut field));
                if fields.len() == 1 && fields[0].is_empty() && !after_quote {
                    return Err(err(line, "empty line"));
                }
                records.push((record_line, std::mem::take(&mut fields)));
                line += 1;
                record_line = line;
                at_field_start = true;
                after_quote = false;
                if records.len() > MAX_ROWS + 1 {
                    return Err(Issue::error(
                        Code::ResourceLimit,
                        format!("more than {MAX_ROWS} data rows"),
                        "Split the data into smaller files.",
                    ));
                }
            }
            '"' if at_field_start => {
                in_quotes = true;
                at_field_start = false;
            }
            '"' => {
                return Err(err(
                    line,
                    "quote inside an unquoted field (quote the whole field and double inner quotes)",
                ));
            }
            _ if after_quote => return Err(err(line, "text after a closing quote")),
            _ => {
                field.push(c);
                at_field_start = false;
            }
        }
    }
    if in_quotes {
        return Err(err(record_line, "quoted field is not closed"));
    }
    if !fields.is_empty() || !field.is_empty() || after_quote {
        fields.push(field);
        records.push((record_line, fields));
    }

    let mut it = records.into_iter();
    let Some((_, header)) = it.next() else {
        return Err(Issue::error(Code::DataInvalid, "CSV is empty", "Add a header row and data rows."));
    };
    let mut seen = std::collections::HashSet::new();
    for h in &header {
        if h.is_empty() {
            return Err(err(1, "empty column name"));
        }
        if !seen.insert(h.as_str()) {
            return Err(err(1, format!("duplicate column {h:?}")));
        }
    }
    let records: Vec<_> = it.collect();
    for (ln, r) in &records {
        if r.len() != header.len() {
            return Err(err(*ln, format!("{} fields; the header has {}", r.len(), header.len())));
        }
    }
    Ok(Table { header, records })
}

/// Row ids become part of file names, so they are restricted.
pub fn is_valid_row_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
}
