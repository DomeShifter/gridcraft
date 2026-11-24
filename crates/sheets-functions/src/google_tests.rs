//! Tests for `google.rs`, with the cases from Google's help pages and Sheets' own answers.

use gridcraft_core::{Array, CellError, DateSystem, Value};

use crate::google::{
    Op, array_constrain, average_weighted, binary, countunique, epochtodate, flatten, is_between, is_date, is_date_format, is_email, is_url, join, parser_identity,
    regexextract, regexmatch, split, to_text, unary_percent, uminus, uplus,
};

fn t(s: &str) -> Value {
    Value::text(s)
}
fn n(x: f64) -> Value {
    Value::number(x)
}
fn col(v: Vec<Value>) -> Value {
    Value::from(Array::column(v))
}
fn grid(rows: usize, cols: usize, v: Vec<Value>) -> Value {
    Value::from(Array::new(rows, cols, v).unwrap())
}
/// A result as rows of display text.
fn shown(v: Value) -> Vec<Vec<String>> {
    match v {
        Value::Array(a) => (0..a.rows).map(|r| (0..a.cols).map(|c| a.get(r, c).unwrap().display()).collect()).collect(),
        v => vec![vec![v.display()]],
    }
}
fn row(v: &[&str]) -> Vec<Vec<String>> {
    vec![v.iter().map(|s| s.to_string()).collect()]
}
fn data(v: Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a.data.clone(),
        v => vec![v],
    }
}
const SYS: DateSystem = DateSystem::D1900;

#[test]
fn split_by_each_character_and_whole_delimiter() {
    assert_eq!(shown(split(&t("Alas, poor Yorick"), &t(" "), true, true).unwrap()), row(&["Alas,", "poor", "Yorick"]));
    // Each character of the delimiter splits (Sheets' default).
    assert_eq!(shown(split(&t("a,b;c"), &t(",;"), true, true).unwrap()), row(&["a", "b", "c"]));
    // split_by_each FALSE: only the whole delimiter.
    assert_eq!(shown(split(&t("a,;b,c"), &t(",;"), false, true).unwrap()), row(&["a", "b,c"]));
    // Spaces next to the delimiter stay.
    assert_eq!(shown(split(&t("a, b"), &t(","), true, true).unwrap()), row(&["a", " b"]));
}

#[test]
fn split_empty_pieces_and_numbers() {
    // Two delimiters in a row count as one, unless remove_empty_text is FALSE.
    assert_eq!(shown(split(&t("a,,b"), &t(","), true, true).unwrap()), row(&["a", "b"]));
    assert_eq!(shown(split(&t("a,,b"), &t(","), true, false).unwrap()), row(&["a", "", "b"]));
    // Numbers come back as numbers; a number to split is read as text.
    assert_eq!(data(split(&t("1,2,3"), &t(","), true, true).unwrap()), vec![n(1.0), n(2.0), n(3.0)]);
    assert_eq!(data(split(&n(12.5), &t("."), true, true).unwrap()), vec![n(12.0), n(5.0)]);
    // Not plain numbers: text.
    assert_eq!(data(split(&t("1.2.3-x,e,1/2"), &t(","), true, true).unwrap()), vec![t("1.2.3-x"), t("e"), t("1/2")]);
    // No delimiter found: the text itself, one cell.
    assert_eq!(split(&t("abc"), &t(","), true, true).unwrap(), t("abc"));
    // Only delimiters: an empty cell.
    assert_eq!(split(&t(",,"), &t(","), true, true).unwrap(), Value::Empty);
}

#[test]
fn split_errors_and_columns() {
    assert_eq!(split(&t(""), &t(","), true, true), Err(CellError::Value));
    assert_eq!(split(&Value::Empty, &t(","), true, true), Err(CellError::Value));
    assert_eq!(split(&t("a"), &t(""), true, true), Err(CellError::Value));
    assert_eq!(split(&Value::Error(CellError::NA), &t(","), true, true), Err(CellError::NA));
    // A column (ARRAYFORMULA(SPLIT(A1:A3, ","))): one row each, padded; an empty cell, an empty row.
    let c = col(vec![t("a,b,c"), Value::Empty, t("d")]);
    assert_eq!(shown(split(&c, &t(","), true, true).unwrap()), vec![vec!["a", "b", "c"], vec!["", "", ""], vec!["d", "", ""]]);
    assert_eq!(split(&grid(2, 2, vec![t("a"), t("b"), t("c"), t("d")]), &t(","), true, true), Err(CellError::Value));
}

#[test]
fn join_rows_columns_and_blanks() {
    let v = grid(1, 3, vec![n(1.0), n(2.0), t("1 2 3 4")]);
    assert_eq!(join(&t(" and-a "), &[v]).unwrap(), t("1 and-a 2 and-a 1 2 3 4"));
    let a = grid(1, 3, vec![n(1.0), n(2.0), n(3.0)]);
    let b = col(vec![n(4.0), n(5.0), n(6.0)]);
    assert_eq!(join(&t(","), &[a.clone(), b]).unwrap(), t("1,2,3,4,5,6"));
    // An empty delimiter (JOIN(,{1,2,3})) and empty cells, which are kept.
    assert_eq!(join(&Value::Empty, &[a]).unwrap(), t("123"));
    assert_eq!(join(&t("-"), &[col(vec![t("a"), Value::Empty, t("c")])]).unwrap(), t("a--c"));
    assert_eq!(join(&t("-"), &[Value::Bool(true), n(0.5)]).unwrap(), t("TRUE-0.5"));
}

#[test]
fn join_errors() {
    assert_eq!(join(&t(","), &[grid(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)])]), Err(CellError::Value));
    assert_eq!(join(&t(","), &[col(vec![t("a"), Value::Error(CellError::Div0)])]), Err(CellError::Div0));
    assert_eq!(join(&Value::Error(CellError::NA), &[t("a")]), Err(CellError::NA));
}

#[test]
fn regexmatch_text_only() {
    assert_eq!(regexmatch(&t("Spreadsheets"), &t("S.r")).unwrap(), Value::Bool(true));
    assert_eq!(regexmatch(&t("Spreadsheets"), &t("^s")).unwrap(), Value::Bool(false)); // case matters
    assert_eq!(regexmatch(&t("Spreadsheets"), &t("(?i)^s")).unwrap(), Value::Bool(true));
    assert_eq!(regexmatch(&Value::Empty, &t("^$")).unwrap(), Value::Bool(true));
    // Only text: numbers and TRUE/FALSE are #VALUE! (Sheets: "only works with text").
    assert_eq!(regexmatch(&n(12.0), &t("1")), Err(CellError::Value));
    assert_eq!(regexmatch(&Value::Bool(true), &t("T")), Err(CellError::Value));
    // A pattern that isn't valid: #REF!.
    assert_eq!(regexmatch(&t("a"), &t("(")), Err(CellError::Ref));
    assert_eq!(regexmatch(&Value::Error(CellError::NA), &t("a")), Err(CellError::NA));
}

#[test]
fn countunique_counts_distinct_values() {
    // COUNTUNIQUE(1,1,2,3,5,8,13): 6.
    let v: Vec<Value> = [1.0, 1.0, 2.0, 3.0, 5.0, 8.0, 13.0].into_iter().map(n).collect();
    assert_eq!(countunique(&v).unwrap(), n(6.0));
    // Blanks and empty text don't count; 1, "1" and TRUE are three values; case matters.
    let r = grid(2, 3, vec![n(1.0), t("1"), Value::Bool(true), Value::Empty, t(""), n(1.0)]);
    assert_eq!(countunique(&[r, t("a"), t("A"), t("a")]).unwrap(), n(5.0));
    assert_eq!(countunique(&[col(vec![Value::Empty, Value::Empty])]).unwrap(), n(0.0));
    assert_eq!(countunique(&[n(1.0), Value::Error(CellError::NA)]), Err(CellError::NA));
}

#[test]
fn flatten_keeps_order_and_blanks() {
    // FLATTEN(A1:B2, "sample middle", B3:B4): by argument, then row, then column.
    let a = grid(2, 2, vec![t("a"), t("b"), Value::Empty, t("d")]);
    let b = col(vec![n(1.0), n(2.0)]);
    assert_eq!(
        shown(flatten(&[a, t("sample middle"), b]).unwrap()),
        vec![vec!["a"], vec!["b"], vec![""], vec!["d"], vec!["sample middle"], vec!["1"], vec!["2"]]
    );
    assert_eq!(flatten(&[n(7.0)]).unwrap(), n(7.0));
    assert_eq!(shown(flatten(&[col(vec![Value::Error(CellError::NA), n(1.0)])]).unwrap()), vec![vec!["#N/A"], vec!["1"]]);
}

#[test]
fn isemail() {
    for ok in ["noreply@google.com", "janesmith@yourname.xyz", "first.last+tag@mail.example.co.uk"] {
        assert_eq!(is_email(&t(ok)).unwrap(), Value::Bool(true), "{ok}");
    }
    for bad in ["", "noreply", "a@b", "a@@b.com", "a b@c.com", ".a@b.com", "a..b@c.com", "a@-b.com", "a@b.c", "a@b.com."] {
        assert_eq!(is_email(&t(bad)).unwrap(), Value::Bool(false), "{bad}");
    }
    assert_eq!(is_email(&n(1.0)).unwrap(), Value::Bool(false));
    assert_eq!(is_email(&Value::Empty).unwrap(), Value::Bool(false));
    assert_eq!(is_email(&Value::Error(CellError::NA)), Err(CellError::NA));
}

#[test]
fn isurl() {
    for ok in [
        "www.google.com",
        "google.com",
        "www.abc.xyz",
        "https://support.google.com/docs/answer/3256501?hl=en#x",
        "http://example.com:8080/",
        "ftp://user@files.example.org/pub",
        "mailto:someone@example.com",
        "HTTP://EXAMPLE.COM",
        "http://10.0.0.1/a",
    ] {
        assert_eq!(is_url(&t(ok)).unwrap(), Value::Bool(true), "{ok}");
    }
    for bad in ["", "google", "http://", "file:///etc/passwd", "javascript:alert(1)", "two words.com", "example.c", "https//example.com"] {
        assert_eq!(is_url(&t(bad)).unwrap(), Value::Bool(false), "{bad}");
    }
    assert_eq!(is_url(&n(3.0)).unwrap(), Value::Bool(false));
    assert_eq!(is_url(&Value::Error(CellError::Ref)), Err(CellError::Ref));
}

#[test]
fn to_text_numbers_only() {
    assert_eq!(to_text(&n(24.0)).unwrap(), t("24"));
    assert_eq!(to_text(&n(0.25)).unwrap(), t("0.25"));
    assert_eq!(to_text(&t("abc")).unwrap(), t("abc"));
    assert_eq!(to_text(&Value::Bool(true)).unwrap(), Value::Bool(true));
    assert_eq!(to_text(&Value::Empty).unwrap(), t(""));
    assert_eq!(to_text(&Value::Error(CellError::NA)), Err(CellError::NA));
}

#[test]
fn arithmetic_operators() {
    assert_eq!(binary(Op::Add, &n(2.0), &n(3.0), SYS).unwrap(), n(5.0));
    assert_eq!(binary(Op::Add, &t("2"), &Value::Bool(true), SYS).unwrap(), n(3.0));
    assert_eq!(binary(Op::Add, &Value::Empty, &n(3.0), SYS).unwrap(), n(3.0));
    assert_eq!(binary(Op::Add, &t("a"), &n(3.0), SYS), Err(CellError::Value));
    assert_eq!(binary(Op::Minus, &n(2.0), &n(3.0), SYS).unwrap(), n(-1.0));
    assert_eq!(binary(Op::Multiply, &n(2.0), &n(3.0), SYS).unwrap(), n(6.0));
    assert_eq!(binary(Op::Divide, &n(4.0), &n(2.0), SYS).unwrap(), n(2.0));
    assert_eq!(binary(Op::Divide, &n(4.0), &n(0.0), SYS), Err(CellError::Div0));
    assert_eq!(binary(Op::Divide, &n(4.0), &Value::Empty, SYS), Err(CellError::Div0));
    // The first error wins.
    assert_eq!(binary(Op::Add, &Value::Error(CellError::NA), &Value::Error(CellError::Div0), SYS), Err(CellError::NA));
}

#[test]
fn pow_follows_sheets() {
    assert_eq!(binary(Op::Pow, &n(4.0), &n(0.5), SYS).unwrap(), n(2.0));
    assert_eq!(binary(Op::Pow, &n(2.0), &n(5.0), SYS).unwrap(), n(32.0));
    assert_eq!(binary(Op::Pow, &n(-2.0), &n(3.0), SYS).unwrap(), n(-8.0));
    assert_eq!(binary(Op::Pow, &n(-8.0), &n(1.0 / 3.0), SYS), Err(CellError::Num));
    assert_eq!(binary(Op::Pow, &n(0.0), &n(0.0), SYS).unwrap(), n(1.0));
    assert_eq!(binary(Op::Pow, &n(0.0), &n(-1.0), SYS), Err(CellError::Div0));
    assert_eq!(binary(Op::Pow, &n(10.0), &n(400.0), SYS), Err(CellError::Num));
}

#[test]
fn comparison_operators() {
    let yes = Value::Bool(true);
    let no = Value::Bool(false);
    assert_eq!(binary(Op::Eq, &n(2.0), &n(3.0), SYS).unwrap(), no);
    assert_eq!(binary(Op::Eq, &t("abc"), &t("ABC"), SYS).unwrap(), yes); // case ignored, as `=`
    assert_eq!(binary(Op::Eq, &Value::Empty, &n(0.0), SYS).unwrap(), yes);
    assert_eq!(binary(Op::Eq, &Value::Empty, &t(""), SYS).unwrap(), yes);
    assert_eq!(binary(Op::Eq, &t("1"), &n(1.0), SYS).unwrap(), no);
    assert_eq!(binary(Op::Ne, &n(2.0), &n(3.0), SYS).unwrap(), yes);
    assert_eq!(binary(Op::Gt, &n(3.0), &n(2.0), SYS).unwrap(), yes);
    assert_eq!(binary(Op::Gte, &n(2.0), &n(2.0), SYS).unwrap(), yes);
    assert_eq!(binary(Op::Lt, &t("a"), &t("b"), SYS).unwrap(), yes);
    assert_eq!(binary(Op::Lte, &n(3.0), &n(2.0), SYS).unwrap(), no);
    // Text sorts after numbers.
    assert_eq!(binary(Op::Gt, &t("a"), &n(1000.0), SYS).unwrap(), yes);
    assert_eq!(binary(Op::Eq, &n(1.0), &Value::Error(CellError::Ref), SYS), Err(CellError::Ref));
}

#[test]
fn unary_operators() {
    assert_eq!(uminus(&n(-4.0), SYS).unwrap(), n(4.0));
    assert_eq!(uminus(&Value::Empty, SYS).unwrap(), n(0.0));
    assert_eq!(uminus(&t("x"), SYS), Err(CellError::Value));
    assert_eq!(uplus(&n(-4.0)).unwrap(), n(-4.0));
    assert_eq!(uplus(&t("x")).unwrap(), t("x"));
    assert_eq!(uplus(&Value::Error(CellError::NA)), Err(CellError::NA));
    assert_eq!(unary_percent(&n(100.0), SYS).unwrap(), n(1.0));
    assert_eq!(unary_percent(&n(93.0), SYS).unwrap(), n(0.93));
    assert_eq!(unary_percent(&t("x"), SYS), Err(CellError::Value));
}

// ---------------------------------------------------------------------------------------------
// Batch 2 (StarSheets 0091)

#[test]
fn regexextract_googles_examples() {
    // "returns 241 because it returns the first matching case".
    assert_eq!(regexextract(&t("My favorite number is 241, but my friend's is 17"), &t("\\d+")).unwrap(), t("241"));
    // Several capture groups: one cell each, side by side.
    let v = regexextract(&t("You can also extract multiple values from text."), &t("You can also (\\w+) multiple (\\w+) from text.")).unwrap();
    assert_eq!(shown(v), row(&["extract", "values"]));
}

#[test]
fn regexextract_groups_and_errors() {
    // One group: its text, not the whole match.
    assert_eq!(regexextract(&t("Order #1234 shipped"), &t("#(\\d+)")).unwrap(), t("1234"));
    // A group that took no part is empty text.
    assert_eq!(data(regexextract(&t("ab"), &t("(a)(x)?(b)")).unwrap()), vec![t("a"), t(""), t("b")]);
    // A non-capturing group doesn't count.
    assert_eq!(regexextract(&t("abc"), &t("(?:a)b")).unwrap(), t("ab"));
    // No match: #N/A; text only; a pattern that isn't valid: #REF!.
    assert_eq!(regexextract(&t("abc"), &t("\\d")), Err(CellError::NA));
    assert_eq!(regexextract(&n(241.0), &t("\\d+")), Err(CellError::Value));
    assert_eq!(regexextract(&Value::Bool(true), &t("T")), Err(CellError::Value));
    assert_eq!(regexextract(&t("a"), &t("(")), Err(CellError::Ref));
    assert_eq!(regexextract(&Value::Error(CellError::Div0), &t("a")), Err(CellError::Div0));
}

#[test]
fn array_constrain_takes_the_top_left() {
    let g = grid(3, 3, (1..=9).map(|i| n(i as f64)).collect());
    assert_eq!(data(array_constrain(&g, 2.0, 2.0).unwrap()), vec![n(1.0), n(2.0), n(4.0), n(5.0)]);
    // Bigger than the array: all of it; truncated counts; 1 × 1 is one value.
    assert_eq!(data(array_constrain(&g, 10.0, 2.9).unwrap()).len(), 6);
    assert_eq!(array_constrain(&g, 1.0, 1.0).unwrap(), n(1.0));
    assert_eq!(array_constrain(&n(5.0), 3.0, 3.0).unwrap(), n(5.0));
    assert_eq!(array_constrain(&g, 0.0, 2.0), Err(CellError::Num));
    assert_eq!(array_constrain(&g, 2.0, 0.5), Err(CellError::Num));
}

#[test]
fn is_between_googles_examples() {
    let yes = |v: R| assert_eq!(v.unwrap(), Value::Bool(true));
    let no = |v: R| assert_eq!(v.unwrap(), Value::Bool(false));
    yes(is_between(&n(7.9), &n(1.2), &n(12.45), true, true));
    yes(is_between(&n(1.2), &n(1.2), &n(12.45), true, true));
    no(is_between(&n(1.2), &n(1.2), &n(12.45), false, true));
    yes(is_between(&n(12.45), &n(1.2), &n(12.45), true, true));
    no(is_between(&n(12.45), &n(1.2), &n(12.45), true, false));
    yes(is_between(&n(7.9), &n(1.2), &n(12.45), false, false));
    no(is_between(&n(20.0), &n(1.2), &n(12.45), true, true));
    assert_eq!(is_between(&Value::Error(CellError::NA), &n(1.0), &n(2.0), true, true), Err(CellError::NA));
}
type R = Result<Value, CellError>;

#[test]
fn is_date_googles_examples() {
    assert_eq!(is_date(&t("July 20 1969"), false, SYS), Value::Bool(true));
    assert_eq!(is_date(&t("1969-20-07"), false, SYS), Value::Bool(true));
    assert_eq!(is_date(&t("July"), false, SYS), Value::Bool(false));
    assert_eq!(is_date(&t("Feb 30"), false, SYS), Value::Bool(false));
    // A number is a date when its cell shows one.
    assert_eq!(is_date(&n(45_000.0), true, SYS), Value::Bool(true));
    assert_eq!(is_date(&n(45_000.0), false, SYS), Value::Bool(false));
    assert_eq!(is_date(&t("2026-11-24"), false, SYS), Value::Bool(true));
    assert_eq!(is_date(&t("hello"), false, SYS), Value::Bool(false));
    assert_eq!(is_date(&t("12"), false, SYS), Value::Bool(false));
    assert_eq!(is_date(&Value::Empty, false, SYS), Value::Bool(false));
    assert_eq!(is_date(&Value::Error(CellError::NA), false, SYS), Value::Bool(false));
    assert!(is_date_format("yyyy-mm-dd") && is_date_format("m/d/yyyy h:mm") && !is_date_format("h:mm") && !is_date_format("0.00"));
}

#[test]
fn epochtodate_googles_examples() {
    // Google's table: the date-times shown, to the second.
    let at = |v: Value, day: f64, secs: f64| {
        let Value::Number(x) = v else { panic!("{v:?}") };
        let want = day + secs / 86_400.0;
        assert!((x - want).abs() < 0.6 / 86_400.0, "{x} vs {want}");
    };
    at(epochtodate(1_655_906_568_893.0, 2.0, SYS).unwrap(), 44_734.0, 14.0 * 3600.0 + 2.0 * 60.0 + 49.0); // 6/22/2022 14:02:49
    at(epochtodate(1_655_906_710.0, 1.0, SYS).unwrap(), 44_734.0, 14.0 * 3600.0 + 5.0 * 60.0 + 10.0); // 6/22/2022 14:05:10
    assert_eq!(epochtodate(0.0, 2.0, SYS).unwrap(), n(25_569.0)); // 1/1/1970 0:00:00
    at(epochtodate(1_584_033_897.0, 1.0, SYS).unwrap(), 43_902.0, 17.0 * 3600.0 + 24.0 * 60.0 + 57.0); // 3/12/2020 17:24:57
    at(epochtodate(1_656_356_678_000_410.0, 3.0, SYS).unwrap(), 44_739.0, 19.0 * 3600.0 + 4.0 * 60.0 + 38.0); // 6/27/2022 19:04:38
    // Fractions of a millisecond are dropped.
    assert_eq!(epochtodate(1_500.9, 3.0, SYS), epochtodate(1.0, 2.0, SYS));
    assert_eq!(epochtodate(-1.0, 1.0, SYS), Err(CellError::Num));
    assert_eq!(epochtodate(1.0, 4.0, SYS), Err(CellError::Num));
    assert_eq!(epochtodate(0.0, 1.0, DateSystem::D1904).unwrap(), n(24_107.0));
}

#[test]
fn average_weighted_googles_examples() {
    let close = |v: R, want: f64| {
        let Ok(Value::Number(x)) = v else { panic!("{v:?}") };
        assert!((x - want).abs() < 1e-9, "{x} vs {want}");
    };
    let a = col(vec![n(2.0), n(4.0)]);
    let b = col(vec![n(1.0), n(3.0)]);
    close(average_weighted(&[a.clone(), b.clone()]), 3.5);
    close(average_weighted(&[n(2.0), n(10.0), n(4.0), n(15.0)]), 3.2);
    close(average_weighted(&[a.clone(), b.clone(), n(8.0), n(6.0)]), 6.2);
    let grades = col(vec![n(95.0), n(90.0), n(85.0), n(88.0), n(82.0)]);
    let parts = col(vec![n(0.25), n(0.10), n(0.15), n(0.20), n(0.30)]);
    close(average_weighted(&[grades, parts]), 87.7);
    // Cells that aren't numbers leave their pair out.
    close(average_weighted(&[col(vec![n(2.0), t("x"), n(4.0)]), col(vec![n(1.0), n(5.0), n(3.0)])]), 3.5);
    // Errors: sizes differ, negative weight, weights all zero, odd count.
    assert_eq!(average_weighted(&[a.clone(), n(1.0)]), Err(CellError::Value));
    assert_eq!(average_weighted(&[n(1.0), n(-1.0)]), Err(CellError::Num));
    assert_eq!(average_weighted(&[n(1.0), n(0.0)]), Err(CellError::Div0));
    assert_eq!(average_weighted(&[a, b, n(1.0)]), Err(CellError::NA));
    assert_eq!(average_weighted(&[n(1.0), Value::Error(CellError::Ref)]), Err(CellError::Ref));
}

#[test]
fn parser_functions_return_the_value() {
    assert_eq!(parser_identity(&n(25_405.0)).unwrap(), n(25_405.0));
    assert_eq!(parser_identity(&n(0.5)).unwrap(), n(0.5));
    assert_eq!(parser_identity(&t("abc")).unwrap(), t("abc"));
    assert_eq!(parser_identity(&Value::Bool(true)).unwrap(), Value::Bool(true));
    assert_eq!(parser_identity(&Value::Error(CellError::NA)), Err(CellError::NA));
}
