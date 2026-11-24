//! Google Sheets' small functions that Excel doesn't have. Batch 1 (StarSheets 0090): SPLIT,
//! JOIN, REGEXMATCH, COUNTUNIQUE, FLATTEN, ISEMAIL, ISURL, TO_TEXT and the operator functions (ADD,
//! MINUS, MULTIPLY, DIVIDE, POW, EQ, NE, GT, GTE, LT, LTE, UMINUS, UPLUS, UNARY_PERCENT). Batch 2
//! (StarSheets 0091): Google's two-argument REGEXEXTRACT and the text rule of REGEXREPLACE,
//! ARRAY_CONSTRAIN, ISBETWEEN, ISDATE, EPOCHTODATE, AVERAGE.WEIGHTED, TO_DATE, TO_DOLLARS,
//! TO_PERCENT and TO_PURE_NUMBER.
//!
//! Each takes values already worked out and returns a value, so it is tested here on its own;
//! `gridcraft-functions` registers them (`sheets.rs`). The operator functions take one value per
//! parameter: the registry applies them cell by cell over ranges, as ARRAYFORMULA does in Sheets.
//! Behaviour follows Google's help pages; where those are silent, Sheets' observed behaviour.

use std::collections::HashSet;
use std::sync::OnceLock;

use gridcraft_core::{Array, CellError, DateSystem, Value, compare};
use gridcraft_numfmt::{FormatKind, NumberFormat};

type R<T> = Result<T, CellError>;

/// Longest text a cell holds (as Excel and GridCraft).
const MAX_TEXT: usize = 32_767;
/// Most cells one result may have (as GridCraft's function library).
const MAX_CELLS: usize = 10_000_000;

fn array(rows: usize, cols: usize, data: Vec<Value>) -> R<Value> {
    if rows.checked_mul(cols).is_none_or(|n| n > MAX_CELLS) {
        return Err(CellError::Num);
    }
    if rows == 1 && cols == 1 {
        return Ok(data.into_iter().next().unwrap_or(Value::Empty));
    }
    Array::new(rows, cols, data).map(Value::from).ok_or(CellError::Calc)
}

fn cells(v: &Value) -> &[Value] {
    match v {
        Value::Array(a) => &a.data,
        v => std::slice::from_ref(v),
    }
}

// ---------------------------------------------------------------------------------------------
// SPLIT

/// SPLIT(text, delimiter, [split_by_each], [remove_empty_text]) for one text: the pieces in a row.
/// - `split_by_each` (Sheets' default TRUE): every character of `delimiter` splits; FALSE: only
///   the whole delimiter does.
/// - `remove_empty_text` (default TRUE): no empty pieces (two delimiters in a row count as one).
/// - A piece that is a plain number becomes a number, as Sheets does (`SPLIT("1,2", ",")` gives
///   1 and 2); anything else stays text, spaces included.
/// - Empty text or an empty delimiter is `#VALUE!` (Sheets: "parameter … should be non-empty").
pub fn split_one(text: &Value, delimiter: &Value, split_by_each: bool, remove_empty: bool) -> R<Value> {
    let s = text.to_text()?;
    let d = delimiter.to_text()?;
    if s.is_empty() || d.is_empty() {
        return Err(CellError::Value);
    }
    let pieces: Vec<&str> = if split_by_each { s.split(|c| d.contains(c)).collect() } else { s.split(d.as_str()).collect() };
    let out: Vec<Value> = pieces.into_iter().filter(|p| !(remove_empty && p.is_empty())).map(piece).collect();
    if out.is_empty() {
        // Nothing but delimiters: Sheets shows an empty cell.
        return Ok(Value::Empty);
    }
    let n = out.len();
    array(1, n, out)
}

/// A piece of SPLIT's result: a plain number ("12", "-3.5", "1e3") becomes a number.
fn piece(p: &str) -> Value {
    let t = p.trim();
    let plain = !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+' | b'e' | b'E'));
    match t.parse::<f64>() {
        Ok(n) if plain && n.is_finite() && t.bytes().any(|b| b.is_ascii_digit()) => Value::number(n),
        _ => Value::text(p),
    }
}

/// SPLIT over its first argument: one text gives one row; a column of texts (ARRAYFORMULA, a
/// range) gives one row per text, padded with empty cells to the widest. An empty cell in a
/// column gives an empty row. A block wider than one column is `#VALUE!`.
pub fn split(text: &Value, delimiter: &Value, split_by_each: bool, remove_empty: bool) -> R<Value> {
    let Value::Array(a) = text else { return split_one(text, delimiter, split_by_each, remove_empty) };
    if a.rows == 1 && a.cols == 1 {
        return split_one(&a.data[0], delimiter, split_by_each, remove_empty);
    }
    if a.cols != 1 {
        return Err(CellError::Value);
    }
    let mut rows: Vec<Vec<Value>> = Vec::with_capacity(a.rows);
    for v in &a.data {
        rows.push(match v {
            Value::Empty => vec![Value::Empty],
            Value::Text(t) if t.is_empty() => vec![Value::Empty],
            v => match split_one(v, delimiter, split_by_each, remove_empty) {
                Ok(Value::Array(r)) => r.data.clone(),
                Ok(one) => vec![one],
                Err(e) => vec![Value::Error(e)],
            },
        });
    }
    let width = rows.iter().map(Vec::len).max().unwrap_or(1);
    let height = rows.len();
    let mut data = Vec::with_capacity(height.saturating_mul(width).min(MAX_CELLS));
    for mut r in rows {
        r.resize(width, Value::Empty);
        data.extend(r);
        if data.len() > MAX_CELLS {
            return Err(CellError::Num);
        }
    }
    array(height, width, data)
}

// ---------------------------------------------------------------------------------------------
// JOIN

/// JOIN(delimiter, value_or_array1, …): every value of every argument, empty cells included,
/// joined by `delimiter`. Each array must be one row or one column (`#VALUE!` otherwise, Sheets:
/// "requires a single row or column"). An error value anywhere is the result.
pub fn join(delimiter: &Value, values: &[Value]) -> R<Value> {
    let d = delimiter.to_text()?;
    let mut out = String::new();
    let mut first = true;
    for v in values {
        if let Value::Array(a) = v
            && a.rows != 1
            && a.cols != 1
        {
            return Err(CellError::Value);
        }
        for x in cells(v) {
            if !first {
                out.push_str(&d);
            }
            first = false;
            out.push_str(&x.to_text()?);
            if out.len() > MAX_TEXT * 4 {
                return Err(CellError::Value);
            }
        }
    }
    Ok(Value::text(out))
}

// ---------------------------------------------------------------------------------------------
// REGEXMATCH

/// REGEXMATCH(text, regular_expression): whether the expression matches anywhere in the text,
/// case-sensitively (RE2 syntax). Sheets only takes text: a number or TRUE/FALSE is `#VALUE!` (an
/// empty cell is empty text); an expression that isn't valid is `#REF!`, as in Sheets.
pub fn regexmatch(text: &Value, pattern: &Value) -> R<Value> {
    let s = regex_text(text)?;
    Ok(Value::Bool(regex(pattern)?.is_match(&s)))
}

/// The text Google's regular expression functions take: text only ("only works with text (not
/// numbers) as input"), so a number or TRUE/FALSE is `#VALUE!`; an empty cell is empty text.
pub fn regex_text(v: &Value) -> R<String> {
    match v {
        Value::Text(t) => Ok(t.to_string()),
        Value::Empty => Ok(String::new()),
        Value::Error(e) => Err(*e),
        _ => Err(CellError::Value),
    }
}

/// A regular expression as Google's functions read it (RE2 syntax, case-sensitive); one that isn't
/// valid is `#REF!`, as Sheets shows it.
pub fn regex(pattern: &Value) -> R<regex::Regex> {
    let p = pattern.to_text()?;
    if p.len() > MAX_TEXT {
        return Err(CellError::Value);
    }
    regex::RegexBuilder::new(&p).size_limit(1 << 22).dfa_size_limit(1 << 22).nest_limit(100).build().map_err(|_| CellError::Ref)
}

/// Google's REGEXEXTRACT(text, regular_expression), the two-argument form. (Excel's own
/// REGEXEXTRACT takes a return mode as its third argument; with that given, Excel's rules apply.)
/// - no capture groups: the first match ("the function returns the whole match");
/// - one capture group: what it caught in the first match;
/// - several: each group's text in a row of cells ("extract" and "values" in Google's example);
///   a group that took no part is empty text.
///
/// No match is `#N/A`; text only, as REGEXMATCH.
pub fn regexextract(text: &Value, pattern: &Value) -> R<Value> {
    let s = regex_text(text)?;
    let re = regex(pattern)?;
    let caps = re.captures(&s).ok_or(CellError::NA)?;
    let group = |i: usize| Value::text(caps.get(i).map_or("", |m| m.as_str()));
    match caps.len() {
        0 | 1 => Ok(group(0)),
        2 => Ok(group(1)),
        n => array(1, n - 1, (1..n).map(group).collect()),
    }
}

// ---------------------------------------------------------------------------------------------
// COUNTUNIQUE and FLATTEN

/// COUNTUNIQUE(value1, …): how many different values the arguments hold. Empty cells and empty
/// text don't count; a number and the same number as text are different, and so are TRUE and 1;
/// text is compared exactly (case matters, as Sheets' UNIQUE). An error value is the result.
pub fn countunique(values: &[Value]) -> R<Value> {
    #[derive(Hash, PartialEq, Eq)]
    enum Key {
        Num(u64),
        Text(String),
        Bool(bool),
    }
    let mut seen: HashSet<Key> = HashSet::new();
    for v in values {
        for x in cells(v) {
            let k = match x {
                Value::Empty => continue,
                Value::Text(t) if t.is_empty() => continue,
                Value::Number(n) => Key::Num((if *n == 0.0 { 0.0 } else { *n }).to_bits()),
                Value::Text(t) => Key::Text(t.to_string()),
                Value::Bool(b) => Key::Bool(*b),
                Value::Error(e) => return Err(*e),
                Value::Array(_) => continue,
            };
            seen.insert(k);
        }
    }
    Ok(Value::number(seen.len() as f64))
}

/// FLATTEN(range1, …): every value in one column, by argument, then row, then column. Empty cells
/// are kept (Sheets: "Empty values are not skipped"); error values are kept as values.
pub fn flatten(values: &[Value]) -> R<Value> {
    let mut out = Vec::new();
    for v in values {
        out.extend(cells(v).iter().cloned());
        if out.len() > MAX_CELLS {
            return Err(CellError::Num);
        }
    }
    let n = out.len();
    if n == 0 {
        return Ok(Value::Empty);
    }
    array(n, 1, out)
}

// ---------------------------------------------------------------------------------------------
// ISEMAIL, ISURL, TO_TEXT

fn email_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // A fixed pattern, checked by the tests.
        regex::Regex::new(r"^[A-Za-z0-9!#$%&'*+/=?^_`{|}~-]+(\.[A-Za-z0-9!#$%&'*+/=?^_`{|}~-]+)*@([A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?\.)+[A-Za-z]{2,63}$").unwrap()
    })
}

fn url_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // A fixed pattern, checked by the tests.
        regex::Regex::new(
            r"(?i)^((https?|ftp|gopher|news|telnet|aim)://([^\s@/]+@)?|mailto:[^\s@/]+@)?(([a-z0-9]([a-z0-9-]*[a-z0-9])?\.)+[a-z]{2,63}|\d{1,3}(\.\d{1,3}){3})(:\d{1,5})?([/?#]\S*)?$",
        )
        .unwrap()
    })
}

/// ISEMAIL(value): whether the value is text in the usual shape of an e-mail address
/// (name@domain.tld). Anything that isn't text is FALSE; an error value is the result.
pub fn is_email(v: &Value) -> R<Value> {
    match v {
        Value::Error(e) => Err(*e),
        Value::Text(t) => Ok(Value::Bool(t.len() <= 254 && email_re().is_match(t))),
        _ => Ok(Value::Bool(false)),
    }
}

/// ISURL(value): whether the value is text shaped like a URL. The protocol is optional (Sheets:
/// "A fully qualified URL is not required") and one of ftp, http, https, gopher, mailto, news,
/// telnet and aim; the host needs a dot and a top-level domain (or is an IPv4 address).
pub fn is_url(v: &Value) -> R<Value> {
    match v {
        Value::Error(e) => Err(*e),
        Value::Text(t) => Ok(Value::Bool(t.len() <= 2048 && url_re().is_match(t))),
        _ => Ok(Value::Bool(false)),
    }
}

/// TO_TEXT(value): a number as text, anything else as it is (an empty cell gives empty text).
/// Sheets keeps the cell's number format; a function sees only the value, so a number here is
/// written in General format (`number_to_text`).
pub fn to_text(v: &Value) -> R<Value> {
    match v {
        Value::Number(_) => Ok(Value::text(v.to_text()?)),
        Value::Empty => Ok(Value::text("")),
        Value::Error(e) => Err(*e),
        v => Ok(v.clone()),
    }
}

// ---------------------------------------------------------------------------------------------
// Operator functions

/// Google's operator functions; each gives what its operator gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// ADD: `+`
    Add,
    /// MINUS: `-`
    Minus,
    /// MULTIPLY: `*`
    Multiply,
    /// DIVIDE: `/`
    Divide,
    /// POW: `^` (POWER)
    Pow,
    /// EQ: `=`
    Eq,
    /// NE: `<>`
    Ne,
    /// GT: `>`
    Gt,
    /// GTE: `>=`
    Gte,
    /// LT: `<`
    Lt,
    /// LTE: `<=`
    Lte,
}

/// The operator function `op` on two values. Arithmetic reads numbers as `+` does (an empty cell
/// is 0, TRUE is 1, numeric text counts, other text is `#VALUE!`); comparisons compare as `=` does
/// (text ignores case, an empty cell equals 0, "" and FALSE). The first error value is the result.
pub fn binary(op: Op, a: &Value, b: &Value, sys: DateSystem) -> R<Value> {
    if let Value::Error(e) = a {
        return Err(*e);
    }
    if let Value::Error(e) = b {
        return Err(*e);
    }
    use std::cmp::Ordering::*;
    let cmp = || compare(a, b);
    let n = |x: &Value| x.to_number_in(sys);
    let v = match op {
        Op::Add => Value::number(n(a)? + n(b)?),
        Op::Minus => Value::number(n(a)? - n(b)?),
        Op::Multiply => Value::number(n(a)? * n(b)?),
        Op::Divide => {
            let (x, y) = (n(a)?, n(b)?);
            if y == 0.0 {
                return Err(CellError::Div0);
            }
            Value::number(x / y)
        }
        Op::Pow => Value::number(pow(n(a)?, n(b)?)?),
        Op::Eq => Value::Bool(cmp() == Equal),
        Op::Ne => Value::Bool(cmp() != Equal),
        Op::Gt => Value::Bool(cmp() == Greater),
        Op::Gte => Value::Bool(cmp() != Less),
        Op::Lt => Value::Bool(cmp() == Less),
        Op::Lte => Value::Bool(cmp() != Greater),
    };
    match v {
        Value::Error(e) => Err(e),
        v => Ok(v),
    }
}

/// Sheets' POW/POWER: 0^0 is 1 (Excel: #NUM!), 0 to a negative power is #DIV/0!, a negative base
/// needs a whole exponent (#NUM! otherwise).
fn pow(x: f64, y: f64) -> R<f64> {
    if x == 0.0 && y < 0.0 {
        return Err(CellError::Div0);
    }
    if x < 0.0 && y.fract() != 0.0 {
        return Err(CellError::Num);
    }
    let r = x.powf(y);
    if r.is_finite() { Ok(r) } else { Err(CellError::Num) }
}

/// UMINUS(value): the number with its sign reversed.
pub fn uminus(v: &Value, sys: DateSystem) -> R<Value> {
    Ok(Value::number(-v.to_number_in(sys)?))
}

/// UPLUS(value): the value unchanged (as a leading `+`), text included.
pub fn uplus(v: &Value) -> R<Value> {
    match v {
        Value::Error(e) => Err(*e),
        Value::Array(a) => Ok(a.data.first().cloned().unwrap_or(Value::Empty)),
        v => Ok(v.clone()),
    }
}

/// UNARY_PERCENT(percentage): the number divided by 100 (`UNARY_PERCENT(100)` is 1), as a
/// trailing `%` does.
pub fn unary_percent(v: &Value, sys: DateSystem) -> R<Value> {
    Ok(Value::number(v.to_number_in(sys)? / 100.0))
}

// ---------------------------------------------------------------------------------------------
// Batch 2 (StarSheets 0091): ARRAY_CONSTRAIN, ISBETWEEN, ISDATE, EPOCHTODATE, AVERAGE.WEIGHTED and
// the parser functions

/// ARRAY_CONSTRAIN(input_range, num_rows, num_cols): the top-left `num_rows` × `num_cols` of an
/// array (all of it where it is smaller). Counts are truncated; less than 1 is `#NUM!`.
pub fn array_constrain(v: &Value, rows: f64, cols: f64) -> R<Value> {
    let (rows, cols) = (rows.trunc(), cols.trunc());
    if rows.is_nan() || cols.is_nan() || rows < 1.0 || cols < 1.0 {
        return Err(CellError::Num);
    }
    let Value::Array(a) = v else { return Ok(v.clone()) };
    let r = a.rows.min(rows.min(MAX_CELLS as f64) as usize);
    let c = a.cols.min(cols.min(MAX_CELLS as f64) as usize);
    let mut data = Vec::with_capacity(r * c);
    for i in 0..r {
        data.extend_from_slice(&a.data[i * a.cols..i * a.cols + c]);
    }
    array(r, c, data)
}

/// ISBETWEEN(value_to_compare, lower_value, upper_value, [lower_value_is_inclusive],
/// [upper_value_is_inclusive]): whether the value lies between the two, each end included unless
/// its flag is FALSE (both default to TRUE). Values compare as `<` and `>` do (numbers and dates;
/// text without case). An error value is the result.
pub fn is_between(v: &Value, lo: &Value, hi: &Value, lo_inclusive: bool, hi_inclusive: bool) -> R<Value> {
    for x in [v, lo, hi] {
        if let Value::Error(e) = x {
            return Err(*e);
        }
    }
    use std::cmp::Ordering::*;
    let above = match compare(v, lo) {
        Greater => true,
        Equal => lo_inclusive,
        Less => false,
    };
    let below = match compare(v, hi) {
        Less => true,
        Equal => hi_inclusive,
        Greater => false,
    };
    Ok(Value::Bool(above && below))
}

/// ISDATE(value): whether the value is a date.
/// - A number is a date when its cell shows it as one (`date_formatted`, from the cell's number
///   format: the evaluator knows it, a value alone doesn't).
/// - Text is a date when it reads as one, as typing it would ("July 20 1969"; not "July" or
///   "Feb 30"). Google's example `ISDATE("1969-20-07")` is TRUE, so a year-first date whose
///   middle part can't be a month is also read as year, day, month.
/// - Anything else, error values included, is FALSE.
pub fn is_date(v: &Value, date_formatted: bool, sys: DateSystem) -> Value {
    Value::Bool(match v {
        Value::Number(_) => date_formatted,
        Value::Text(t) => text_is_date(t, sys),
        _ => false,
    })
}

/// Whether a number format shows a calendar date (`yyyy-mm-dd`, `m/d/yyyy h:mm`; not `h:mm`).
pub fn is_date_format(code: &str) -> bool {
    NumberFormat::parse(code).kind() == FormatKind::Date
}

fn text_is_date(t: &str, sys: DateSystem) -> bool {
    let reads = |t: &str| {
        let p = gridcraft_core::parse::parse_input(t, sys);
        matches!((p.value, p.format), (Value::Number(_), Some(f)) if is_date_format(f))
    };
    let t = t.trim();
    if t.is_empty() || t.len() > 64 {
        return false;
    }
    if reads(t) {
        return true;
    }
    // yyyy-dd-mm (Google's own example).
    let parts: Vec<&str> = t.split(['-', '/', '.']).collect();
    if let [y, d, m] = parts[..]
        && y.len() == 4
        && d.parse::<u32>().is_ok_and(|d| d > 12)
        && m.parse::<u32>().is_ok()
    {
        return reads(&format!("{y}-{m}-{d}"));
    }
    false
}

/// Days from the spreadsheet's day 0 to 1 January 1970, the Unix epoch (1900 date system).
const EPOCH_1900: f64 = 25_569.0;
/// The same in the 1904 date system.
const EPOCH_1904: f64 = 24_107.0;
/// The serial number of 31 December 9999, the last date a cell shows (1900 date system).
const LAST_SERIAL: f64 = 2_958_465.0;

/// EPOCHTODATE(timestamp, [unit]): a Unix timestamp as a date-time in UTC. `unit` 1 (the default)
/// is seconds, 2 milliseconds, 3 microseconds; fractions of a millisecond are dropped. A negative
/// timestamp is `#NUM!` ("Negative timestamps aren't accepted"), and so is another unit.
pub fn epochtodate(timestamp: f64, unit: f64, sys: DateSystem) -> R<Value> {
    if timestamp < 0.0 {
        return Err(CellError::Num);
    }
    let ms = match unit.trunc() as i64 {
        1 => timestamp * 1000.0,
        2 => timestamp,
        3 => timestamp / 1000.0,
        _ => return Err(CellError::Num),
    }
    .trunc();
    let day0 = match sys {
        DateSystem::D1900 => EPOCH_1900,
        DateSystem::D1904 => EPOCH_1904,
    };
    let serial = ms / 86_400_000.0 + day0;
    if serial.is_finite() && serial < LAST_SERIAL + 1.0 { Ok(Value::number(serial)) } else { Err(CellError::Num) }
}

/// AVERAGE.WEIGHTED(values, weights, [additional values, additional weights] …): the sum of each
/// value times its weight, divided by the sum of the weights.
/// - Arguments come in pairs (an odd count is `#N/A`, Sheets' "wrong number of arguments"); a
///   pair's values and weights have the same rows and columns (`#VALUE!` otherwise).
/// - A value or weight that isn't a number (an empty cell, text) leaves its pair out.
/// - A negative weight is `#NUM!` ("Weights cannot be negative"); weights adding up to 0 are
///   `#DIV/0!` ("At least one of the weights must be positive"). An error value is the result.
pub fn average_weighted(args: &[Value]) -> R<Value> {
    if args.len() < 2 || !args.len().is_multiple_of(2) {
        return Err(CellError::NA);
    }
    let shape = |v: &Value| match v {
        Value::Array(a) => (a.rows, a.cols),
        _ => (1, 1),
    };
    let (mut sum, mut weights) = (0.0, 0.0);
    for pair in args.chunks(2) {
        let [values, w] = pair else { break };
        if shape(values) != shape(w) {
            return Err(CellError::Value);
        }
        for (x, y) in cells(values).iter().zip(cells(w)) {
            if let Value::Error(e) = x {
                return Err(*e);
            }
            if let Value::Error(e) = y {
                return Err(*e);
            }
            let (Value::Number(x), Value::Number(y)) = (x, y) else { continue };
            if *y < 0.0 {
                return Err(CellError::Num);
            }
            sum += x * y;
            weights += y;
        }
    }
    if weights == 0.0 {
        return Err(CellError::Div0);
    }
    let r = sum / weights;
    if r.is_finite() { Ok(Value::number(r)) } else { Err(CellError::Num) }
}

/// TO_DATE, TO_DOLLARS, TO_PERCENT and TO_PURE_NUMBER(value): a number comes back as it is, and
/// so does anything else ("returned without modification"). In Sheets the first three also show
/// the number as a date, dollars or a percentage, and the last drops the formatting; a function's
/// result carries no number format here, so the cell's own format shows it.
pub fn parser_identity(v: &Value) -> R<Value> {
    match v {
        Value::Error(e) => Err(*e),
        Value::Array(a) => Ok(a.data.first().cloned().unwrap_or(Value::Empty)),
        v => Ok(v.clone()),
    }
}
