use gridcraft_core::{Array, Value};

use super::query;

/// The test sheet: a header row and six people.
fn staff() -> Array {
    let rows: Vec<Vec<Value>> = vec![
        vec![t("Name"), t("Dept"), t("Salary"), t("Start")],
        vec![t("Ann"), t("Eng"), n(5000.0), n(45292.0)],  // 2024-01-01
        vec![t("Bob"), t("Sales"), n(3000.0), n(44927.0)], // 2023-01-01
        vec![t("Cat"), t("Eng"), n(7000.0), n(45658.0)],  // 2025-01-01
        vec![t("Dan"), t("Ops"), n(2500.0), Value::Empty],
        vec![t("Eve"), t("Sales"), n(4000.0), n(45383.0)], // 2024-04-01
        vec![t("Fay"), t("Eng"), Value::Empty, n(45474.0)], // 2024-07-01
    ];
    let cols = rows[0].len();
    Array::new(rows.len(), cols, rows.into_iter().flatten().collect()).unwrap()
}

fn t(s: &str) -> Value {
    Value::Text(s.into())
}

fn n(x: f64) -> Value {
    Value::number(x)
}

/// The result as text rows, for readable assertions ("" for empty).
fn run(q: &str) -> Vec<Vec<String>> {
    run_with(&staff(), q, None, Some(0))
}

fn run_with(data: &Array, q: &str, headers: Option<i64>, first_col: Option<u32>) -> Vec<Vec<String>> {
    let a = query(data, q, headers, first_col, 46_000.5).unwrap_or_else(|e| panic!("{q}: {e}"));
    (0..a.rows).map(|r| (0..a.cols).map(|c| a.get(r, c).map(Value::display).unwrap_or_default()).collect()).collect()
}

fn rows(v: &[&[&str]]) -> Vec<Vec<String>> {
    v.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect()
}

#[test]
fn select_where_order() {
    assert_eq!(run("select A, C where C > 3500 order by C desc"), rows(&[&["Name", "Salary"], &["Cat", "7000"], &["Ann", "5000"], &["Eve", "4000"]]));
    assert_eq!(run("select A where B = 'Sales'"), rows(&[&["Name"], &["Bob"], &["Eve"]]));
    assert_eq!(run("select * limit 1"), rows(&[&["Name", "Dept", "Salary", "Start"], &["Ann", "Eng", "5000", "45292"]]));
    assert_eq!(run("select A order by A desc limit 2 offset 1"), rows(&[&["Name"], &["Eve"], &["Dan"]]));
}

#[test]
fn text_tests_and_nulls() {
    // Case matters, as in Sheets: "Ann" has no lower-case a.
    assert_eq!(run("select A where A contains 'a'"), rows(&[&["Name"], &["Cat"], &["Dan"], &["Fay"]]));
    assert_eq!(run("select A where A starts with 'B' or A ends with 'e'"), rows(&[&["Name"], &["Bob"], &["Eve"]]));
    assert_eq!(run("select A where A matches '[A-C].*'"), rows(&[&["Name"], &["Ann"], &["Bob"], &["Cat"]]));
    assert_eq!(run("select A where A like '_a%'"), rows(&[&["Name"], &["Cat"], &["Dan"], &["Fay"]]));
    assert_eq!(run("select A where C is null"), rows(&[&["Name"], &["Fay"]]));
    assert_eq!(run("select A where D is not null and not B = 'Eng'"), rows(&[&["Name"], &["Bob"], &["Eve"]]));
}

#[test]
fn totals_and_groups() {
    assert_eq!(run("select B, sum(C) group by B"), rows(&[&["Dept", "sum Salary"], &["Eng", "12000"], &["Ops", "2500"], &["Sales", "7000"]]));
    assert_eq!(run("select B, count(A), avg(C) group by B order by count(A) desc label count(A) 'People'"), rows(&[&["Dept", "People", "avg Salary"], &["Eng", "3", "6000"], &["Sales", "2", "3500"], &["Ops", "1", "2500"]]));
    assert_eq!(run("select max(C), min(A)"), rows(&[&["max Salary", "min Name"], &["7000", "Ann"]]));
}

#[test]
fn pivot() {
    assert_eq!(run("select sum(C) pivot B"), rows(&[&["Eng", "Ops", "Sales"], &["12000", "2500", "7000"]]));
    assert_eq!(run("select year(D), count(A) where D is not null group by year(D) pivot B"), rows(&[&["year(Start)", "Eng", "Sales"], &["2023", "", "1"], &["2024", "2", "1"], &["2025", "1", ""]]));
}

#[test]
fn dates_and_functions() {
    assert_eq!(run("select A where D >= date '2024-04-01' order by D"), rows(&[&["Name"], &["Eve"], &["Fay"], &["Cat"]]));
    assert_eq!(run("select A, month(D), quarter(D), dayOfWeek(D) where A = 'Eve'"), rows(&[&["Name", "month(Start)", "quarter(Start)", "dayofweek(Start)"], &["Eve", "3", "2", "2"]]));
    assert_eq!(run("select upper(A), C * 2 where A = 'Bob'"), rows(&[&["upper(Name)", "Salary*2"], &["BOB", "6000"]]));
}

#[test]
fn columns_by_position_and_headers() {
    // The data starting in column C of the sheet: its first column is C.
    assert_eq!(run_with(&staff(), "select C where E > 6000", None, Some(2)), rows(&[&["Name"], &["Cat"]]));
    // An array (no sheet columns): Col1, Col2…
    assert_eq!(run_with(&staff(), "select Col1 where Col3 > 6000", None, None), rows(&[&["Name"], &["Cat"]]));
    // No header row: nothing on top.
    assert_eq!(run_with(&staff(), "select A where C > 6000", Some(0), Some(0)), rows(&[&["Cat"]]));
}

#[test]
fn errors_say_where() {
    let e = query(&staff(), "select Z", None, Some(0), 0.0).unwrap_err();
    assert!(e.msg.contains("no column"), "{e}");
    let e = query(&staff(), "where C > 1 select A", None, Some(0), 0.0).unwrap_err();
    assert!(e.msg.contains("wrong place"), "{e}");
    let e = query(&staff(), "select A, sum(C)", None, Some(0), 0.0).unwrap_err();
    assert!(e.msg.contains("GROUP BY"), "{e}");
}
