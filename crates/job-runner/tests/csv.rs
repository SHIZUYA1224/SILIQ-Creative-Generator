//! Strict CSV reader: values are kept exactly; anything ambiguous is an error.

use design_model::Code;
use job_runner::csv::{is_valid_row_id, parse};

fn rows(src: &str) -> Vec<Vec<String>> {
    parse(src.as_bytes()).unwrap().records.into_iter().map(|(_, r)| r).collect()
}

#[test]
fn keeps_values_verbatim() {
    let t = parse("rowId,price,tel\nr1,0098,=1+1\n".as_bytes()).unwrap();
    assert_eq!(t.header, vec!["rowId", "price", "tel"]);
    assert_eq!(t.records[0].1, vec!["r1", "0098", "=1+1"]);
    // Spaces are not trimmed; formulas are not evaluated.
    let t2 = parse("rowId,price,tel\nr2, 9800 ,@SUM(A1)\n".as_bytes()).unwrap();
    assert_eq!(t2.records[0].1, vec!["r2", " 9800 ", "@SUM(A1)"]);
}

#[test]
fn quoted_fields_with_commas_quotes_and_newlines() {
    let r = rows("rowId,a\nr1,\"9,800\"\nr2,\"say \"\"hi\"\"\"\nr3,\"line1\nline2\"\n");
    assert_eq!(r[0][1], "9,800");
    assert_eq!(r[1][1], "say \"hi\"");
    assert_eq!(r[2][1], "line1\nline2");
}

#[test]
fn crlf_bom_and_missing_final_newline() {
    let r = rows("\u{feff}rowId,a\r\nr1,x\r\nr2,y");
    assert_eq!(r, vec![vec!["r1", "x"], vec!["r2", "y"]]);
    let t = parse("\u{feff}rowId,a\r\n".as_bytes()).unwrap();
    assert_eq!(t.header[0], "rowId");
}

#[test]
fn empty_fields_are_kept_as_empty() {
    assert_eq!(rows("rowId,a,b\nr1,,\n"), vec![vec!["r1", "", ""]]);
}

#[test]
fn malformed_input_is_rejected_with_line_numbers() {
    for (src, needle) in [
        ("rowId,a\nr1,x,y\n", "line 2"),
        ("rowId,a\nr1,ab\"c\n", "line 2"),
        ("rowId,a\nr1,\"x\"y\n", "line 2"),
        ("rowId,a\nr1,\"open\n", "line 2"),
        ("rowId,a\n\nr1,x\n", "line 2"),
        ("rowId,a\rr1,x\n", "line 1"),
        ("rowId,rowId\nr1,x\n", "line 1"),
        ("rowId,\nr1,x\n", "line 1"),
    ] {
        let e = parse(src.as_bytes()).unwrap_err();
        assert_eq!(e.code, Code::DataInvalid, "{src:?}");
        assert!(e.message.contains(needle), "{src:?}: {}", e.message);
    }
}

#[test]
fn non_utf8_is_rejected_not_guessed() {
    // "店" in Shift_JIS
    let e = parse(b"rowId,a\nr1,\x93\x58\n").unwrap_err();
    assert_eq!(e.code, Code::DataInvalid);
    assert!(e.message.contains("UTF-8"));
}

#[test]
fn empty_input_is_rejected() {
    assert_eq!(parse(b"").unwrap_err().code, Code::DataInvalid);
}

#[test]
fn row_limit_enforced() {
    let mut s = String::from("rowId\n");
    for i in 0..=job_runner::csv::MAX_ROWS {
        s.push_str(&format!("r{i}\n"));
    }
    assert_eq!(parse(s.as_bytes()).unwrap_err().code, Code::ResourceLimit);
}

#[test]
fn row_ids_are_file_name_safe() {
    assert!(is_valid_row_id("store-001"));
    assert!(is_valid_row_id("A_1"));
    for bad in ["", "-a", "../x", "a b", "店舗1", "a/b", &"x".repeat(65)] {
        assert!(!is_valid_row_id(bad), "{bad:?}");
    }
}
