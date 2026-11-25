//! Google Sheets functions that Excel doesn't have. Batch 1 (StarSheets 0090): SPLIT, JOIN,
//! REGEXMATCH, COUNTUNIQUE, FLATTEN, ISEMAIL, ISURL, TO_TEXT and the operator functions. Batch 2
//! (StarSheets 0091): ARRAY_CONSTRAIN, ISBETWEEN, ISDATE, EPOCHTODATE, AVERAGE.WEIGHTED,
//! MARGINOFERROR and the parser functions TO_DATE, TO_DOLLARS, TO_PERCENT, TO_PURE_NUMBER (Google's
//! REGEXEXTRACT and REGEXREPLACE are in `text.rs`, beside Excel's). The work is done in the
//! StarSheets crate `sheets-functions` (`google.rs`, tested there); this registers them. Formulas
//! using them are saved to .xlsx in Google's export wrapper (`__xludf.DUMMYFUNCTION`, xlsx
//! `sheet_write.rs` GOOGLE_ONLY), as QUERY is.

use gridcraft_core::{CellError, Value};
use sheets_functions::google::{self, Op};

use crate::util::{A, R, S, arg, num, numbers, opt_bool, opt_num, scalar};
use crate::{Arg, Ctx, FnSpec, VAR};

fn values(args: &[Arg]) -> Vec<Value> {
    args.iter().map(|a| a.value.clone()).collect()
}

fn split(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    google::split(&arg(a, 0)?.value, &scalar(arg(a, 1)?), opt_bool(a, 2, true)?, opt_bool(a, 3, true)?)
}

fn join(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    google::join(&scalar(arg(a, 0)?), &values(a.get(1..).unwrap_or_default()))
}

fn regexmatch(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    google::regexmatch(&scalar(arg(a, 0)?), &scalar(arg(a, 1)?))
}

fn countunique(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    google::countunique(&values(a))
}

fn flatten(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    google::flatten(&values(a))
}

fn isemail(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    google::is_email(&scalar(arg(a, 0)?))
}

fn isurl(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    google::is_url(&scalar(arg(a, 0)?))
}

fn to_text(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    google::to_text(&scalar(arg(a, 0)?))
}

fn op(op: Op, a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    google::binary(op, &scalar(arg(a, 0)?), &scalar(arg(a, 1)?), c.date_system())
}

fn uminus(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    google::uminus(&scalar(arg(a, 0)?), c.date_system())
}

fn uplus(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    google::uplus(&scalar(arg(a, 0)?))
}

fn unary_percent(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    google::unary_percent(&scalar(arg(a, 0)?), c.date_system())
}

fn array_constrain(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    google::array_constrain(&arg(a, 0)?.value, num(a, 1)?, num(a, 2)?)
}

fn isbetween(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    google::is_between(&scalar(arg(a, 0)?), &scalar(arg(a, 1)?), &scalar(arg(a, 2)?), opt_bool(a, 3, true)?, opt_bool(a, 4, true)?)
}

/// ISDATE of a value alone. The evaluator answers ISDATE of a cell itself (calc `isdate`), where
/// the cell's number format says whether a number is a date.
fn isdate(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    Ok(google::is_date(&scalar(arg(a, 0)?), false, c.date_system()))
}

fn epochtodate(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    google::epochtodate(num(a, 0)?, opt_num(a, 1, 1.0)?, c.date_system())
}

/// AVERAGE.WEIGHTED: a value typed in the formula is read as a number ("3" is 3, other text is
/// `#VALUE!`); in ranges, cells that aren't numbers leave their pair out (`google.rs`).
fn average_weighted(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let mut v = Vec::with_capacity(a.len());
    for x in a {
        v.push(match &x.value {
            Value::Array(_) | Value::Number(_) | Value::Empty | Value::Error(_) => x.value.clone(),
            _ if x.from_ref => x.value.clone(),
            other => Value::number(other.to_number()?),
        });
    }
    google::average_weighted(&v)
}

/// MARGINOFERROR(range, confidence): CONFIDENCE.T(1 - confidence, STDEV(range), COUNT(range)), as
/// Google's help page defines it. The confidence is between 0 and 1 (`#NUM!` otherwise); fewer
/// than two numbers is `#DIV/0!` (STDEV's answer).
fn marginoferror(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let xs = numbers(a.get(..1).unwrap_or_default())?;
    let confidence = num(a, 1)?;
    if !(confidence > 0.0 && confidence < 1.0) {
        return Err(CellError::Num);
    }
    let n = xs.len() as f64;
    if xs.len() < 2 {
        return Err(CellError::Div0);
    }
    let mean = xs.iter().sum::<f64>() / n;
    let sd = (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
    let spec = crate::lookup("CONFIDENCE.T").ok_or(CellError::Calc)?;
    match crate::call(spec, &[Arg::val(Value::number(1.0 - confidence)), Arg::val(Value::number(sd)), Arg::val(Value::number(n))], c) {
        Value::Error(e) => Err(e),
        v => Ok(v),
    }
}

/// SPARKLINE and IMAGE (StarSheets 0102): the cell holds no value (empty text, as in Google
/// Sheets); the grid draws the chart or the picture from the formula's arguments.
fn sparkline(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    match &arg(a, 0)?.value {
        Value::Error(e) => Err(*e),
        _ => Ok(Value::text("")),
    }
}

fn image(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    match scalar(arg(a, 0)?) {
        Value::Text(t) if !t.trim().is_empty() => Ok(Value::text("")),
        Value::Error(e) => Err(e),
        _ => Err(CellError::Value),
    }
}

fn parser_identity(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    google::parser_identity(&scalar(arg(a, 0)?))
}

/// SPLIT takes its text whole (a column splits row by row); its other arguments lift.
const SPLIT_ARGS: &[bool] = &[false, true];
/// ARRAY_CONSTRAIN and MARGINOFERROR take their range whole; the numbers after it lift.
const RANGE_THEN_SCALARS: &[bool] = &[false, true];

pub(crate) fn specs() -> Vec<FnSpec> {
    vec![
        f!(
            "SPLIT",
            2,
            4,
            Text,
            SPLIT_ARGS,
            "SPLIT(text, delimiter, [split_by_each], [remove_empty_text])",
            "Splits text around a delimiter into cells side by side (Google Sheets).",
            split
        ),
        f!("JOIN", 2, VAR, Text, A, "JOIN(delimiter, value_or_array1, [value_or_array2, ...])", "Joins the values of one-row or one-column arrays with a delimiter (Google Sheets).", join),
        f!("REGEXMATCH", 2, 2, Text, S, "REGEXMATCH(text, regular_expression)", "TRUE when a regular expression matches part of the text (Google Sheets).", regexmatch),
        f!("COUNTUNIQUE", 1, VAR, MathTrig, A, "COUNTUNIQUE(value1, [value2, ...])", "Counts the different values in a list of values and ranges (Google Sheets).", countunique),
        f!("FLATTEN", 1, VAR, Lookup, A, "FLATTEN(range1, [range2, ...])", "Puts all the values of one or more ranges in a single column (Google Sheets).", flatten),
        f!("ISEMAIL", 1, 1, Information, S, "ISEMAIL(value)", "TRUE when the value is text shaped like an e-mail address (Google Sheets).", isemail),
        f!("ISURL", 1, 1, Information, S, "ISURL(value)", "TRUE when the value is text shaped like a URL (Google Sheets).", isurl),
        f!("TO_TEXT", 1, 1, Text, S, "TO_TEXT(value)", "Turns a number into text; other values are returned as they are (Google Sheets).", to_text),
        f!("ADD", 2, 2, MathTrig, S, "ADD(value1, value2)", "The sum of two numbers, as the + operator (Google Sheets).", |a: &[Arg], c: &mut dyn Ctx| op(Op::Add, a, c)),
        f!("MINUS", 2, 2, MathTrig, S, "MINUS(value1, value2)", "The difference of two numbers, as the - operator (Google Sheets).", |a: &[Arg], c: &mut dyn Ctx| op(Op::Minus, a, c)),
        f!("MULTIPLY", 2, 2, MathTrig, S, "MULTIPLY(factor1, factor2)", "The product of two numbers, as the * operator (Google Sheets).", |a: &[Arg], c: &mut dyn Ctx| op(Op::Multiply, a, c)),
        f!("DIVIDE", 2, 2, MathTrig, S, "DIVIDE(dividend, divisor)", "One number divided by another, as the / operator (Google Sheets).", |a: &[Arg], c: &mut dyn Ctx| op(Op::Divide, a, c)),
        f!("POW", 2, 2, MathTrig, S, "POW(base, exponent)", "A number raised to a power, as the ^ operator (Google Sheets).", |a: &[Arg], c: &mut dyn Ctx| op(Op::Pow, a, c)),
        f!("EQ", 2, 2, Logical, S, "EQ(value1, value2)", "TRUE when two values are equal, as the = operator (Google Sheets).", |a: &[Arg], c: &mut dyn Ctx| op(Op::Eq, a, c)),
        f!("NE", 2, 2, Logical, S, "NE(value1, value2)", "TRUE when two values are not equal, as the <> operator (Google Sheets).", |a: &[Arg], c: &mut dyn Ctx| op(Op::Ne, a, c)),
        f!("GT", 2, 2, Logical, S, "GT(value1, value2)", "TRUE when the first value is greater, as the > operator (Google Sheets).", |a: &[Arg], c: &mut dyn Ctx| op(Op::Gt, a, c)),
        f!("GTE", 2, 2, Logical, S, "GTE(value1, value2)", "TRUE when the first value is greater or equal, as the >= operator (Google Sheets).", |a: &[Arg], c: &mut dyn Ctx| op(Op::Gte, a, c)),
        f!("LT", 2, 2, Logical, S, "LT(value1, value2)", "TRUE when the first value is less, as the < operator (Google Sheets).", |a: &[Arg], c: &mut dyn Ctx| op(Op::Lt, a, c)),
        f!("LTE", 2, 2, Logical, S, "LTE(value1, value2)", "TRUE when the first value is less or equal, as the <= operator (Google Sheets).", |a: &[Arg], c: &mut dyn Ctx| op(Op::Lte, a, c)),
        f!("UMINUS", 1, 1, MathTrig, S, "UMINUS(value)", "The number with its sign reversed (Google Sheets).", uminus),
        f!("UPLUS", 1, 1, MathTrig, S, "UPLUS(value)", "The value unchanged, as a leading + (Google Sheets).", uplus),
        f!("UNARY_PERCENT", 1, 1, MathTrig, S, "UNARY_PERCENT(percentage)", "The number divided by 100, as a trailing % (Google Sheets).", unary_percent),
        f!(
            "ARRAY_CONSTRAIN",
            3,
            3,
            Lookup,
            RANGE_THEN_SCALARS,
            "ARRAY_CONSTRAIN(input_range, num_rows, num_cols)",
            "The top-left rows and columns of an array result (Google Sheets).",
            array_constrain
        ),
        f!(
            "ISBETWEEN",
            3,
            5,
            Logical,
            S,
            "ISBETWEEN(value_to_compare, lower_value, upper_value, [lower_value_is_inclusive], [upper_value_is_inclusive])",
            "TRUE when a value lies between two others, ends included unless set FALSE (Google Sheets).",
            isbetween
        ),
        f!("ISDATE", 1, 1, Information, S, "ISDATE(value)", "TRUE when the value is a date, or text that reads as a date (Google Sheets).", isdate),
        f!(
            "EPOCHTODATE",
            1,
            2,
            DateTime,
            S,
            "EPOCHTODATE(timestamp, [unit])",
            "A Unix timestamp in seconds (1), milliseconds (2) or microseconds (3) as a date-time in UTC (Google Sheets).",
            epochtodate
        ),
        f!(
            "AVERAGE.WEIGHTED",
            2,
            VAR,
            Statistical,
            A,
            "AVERAGE.WEIGHTED(values, weights, [additional_values, additional_weights, ...])",
            "The average of values, each counted by its weight (Google Sheets).",
            average_weighted
        ),
        f!(
            "MARGINOFERROR",
            2,
            2,
            Statistical,
            RANGE_THEN_SCALARS,
            "MARGINOFERROR(range, confidence)",
            "The random sampling error of a range of values at a confidence level (Google Sheets).",
            marginoferror
        ),
        f!(
            "SPARKLINE",
            1,
            2,
            Lookup,
            A,
            "SPARKLINE(data, [options])",
            "A small chart inside the cell: line, bar, column or win/loss; options is a two-column range or array such as {\"charttype\",\"column\";\"color\",\"red\"} (Google Sheets).",
            sparkline
        ),
        f!(
            "IMAGE",
            1,
            4,
            Lookup,
            A,
            "IMAGE(url, [mode], [height], [width])",
            "Shows the picture at a web address inside the cell; mode 1 fits it (default), 2 stretches it, 3 keeps its own size, 4 uses height and width in pixels (Google Sheets).",
            image
        ),
        f!("TO_DATE", 1, 1, Text, S, "TO_DATE(value)", "A number as a date; other values unchanged (Google Sheets).", parser_identity),
        f!("TO_DOLLARS", 1, 1, Text, S, "TO_DOLLARS(value)", "A number as a dollar value; other values unchanged (Google Sheets).", parser_identity),
        f!("TO_PERCENT", 1, 1, Text, S, "TO_PERCENT(value)", "A number as a percentage; other values unchanged (Google Sheets).", parser_identity),
        f!(
            "TO_PURE_NUMBER",
            1,
            1,
            Text,
            S,
            "TO_PURE_NUMBER(value)",
            "A date, percentage or currency as a plain number; other values unchanged (Google Sheets).",
            parser_identity
        ),
    ]
}

#[cfg(test)]
mod tests {
    use gridcraft_core::{Array, CellError, DateSystem, Value};

    use crate::{Arg, Ctx, call, lookup};

    struct C;
    impl Ctx for C {
        fn date_system(&self) -> DateSystem {
            DateSystem::D1900
        }
        fn now_serial(&self) -> f64 {
            0.0
        }
        fn random(&mut self) -> f64 {
            0.5
        }
    }

    fn ev(name: &str, args: Vec<Arg>) -> Value {
        call(lookup(name).unwrap(), &args, &mut C)
    }

    /// Registered under Google's names, with lifting over ranges as ARRAYFORMULA does.
    #[test]
    fn registered_and_lifted() {
        for name in [
            "SPLIT", "JOIN", "REGEXMATCH", "COUNTUNIQUE", "FLATTEN", "ISEMAIL", "ISURL", "TO_TEXT", "ADD", "MINUS", "MULTIPLY", "DIVIDE", "POW", "EQ", "NE", "GT", "GTE", "LT", "LTE",
            "UMINUS", "UPLUS", "UNARY_PERCENT",
        ] {
            assert!(lookup(name).is_some(), "{name}");
        }
        let col = Value::from(Array::column(vec![Value::number(1.0), Value::number(2.0)]));
        let Value::Array(a) = ev("ADD", vec![Arg::reference(col.clone()), Arg::val(Value::number(10.0))]) else { panic!() };
        assert_eq!(a.data, vec![Value::number(11.0), Value::number(12.0)]);
        let Value::Array(a) = ev("SPLIT", vec![Arg::val(Value::text("a,b")), Arg::val(Value::text(","))]) else { panic!() };
        assert_eq!((a.rows, a.cols), (1, 2));
        assert_eq!(ev("SPLIT", vec![Arg::val(Value::text("a,,b")), Arg::val(Value::text(",")), Arg::val(Value::Bool(true)), Arg::val(Value::Bool(false))]).scalar(), Value::text("a"));
        assert_eq!(ev("JOIN", vec![Arg::val(Value::text("-")), Arg::reference(col)]), Value::text("1-2"));
        assert_eq!(ev("DIVIDE", vec![Arg::val(Value::number(1.0)), Arg::val(Value::Empty)]), Value::Error(CellError::Div0));
        assert_eq!(ev("REGEXMATCH", vec![Arg::val(Value::number(1.0)), Arg::val(Value::text("1"))]), Value::Error(CellError::Value));
    }

    /// SPARKLINE and IMAGE (StarSheets 0102): no value in the cell; the grid draws them.
    #[test]
    fn sparkline_and_image_leave_the_cell_empty() {
        let col = Value::from(Array::column(vec![Value::number(1.0), Value::number(3.0)]));
        assert_eq!(ev("SPARKLINE", vec![Arg::reference(col)]), Value::text(""));
        assert_eq!(ev("SPARKLINE", vec![Arg::val(Value::Error(CellError::Ref))]), Value::Error(CellError::Ref));
        assert_eq!(ev("IMAGE", vec![Arg::val(Value::text("https://example.org/a.png"))]), Value::text(""));
        assert_eq!(ev("IMAGE", vec![Arg::val(Value::number(1.0))]), Value::Error(CellError::Value));
    }

    /// Batch 2 (StarSheets 0091): registered, and the ones worked out here rather than in
    /// `sheets-functions`.
    #[test]
    fn batch_2_registered() {
        for name in [
            "ARRAY_CONSTRAIN", "ISBETWEEN", "ISDATE", "EPOCHTODATE", "AVERAGE.WEIGHTED", "MARGINOFERROR", "TO_DATE", "TO_DOLLARS", "TO_PERCENT", "TO_PURE_NUMBER", "IMCOTH",
            "IMLOG", "IMTANH",
        ] {
            assert!(lookup(name).is_some(), "{name}");
        }
        let n = |x: f64| Arg::val(Value::number(x));
        // MARGINOFERROR: Google's example, 8, 4, 3, 6 at 95%: 3.528 (5.25 ± 3.528).
        let data = Value::from(Array::column(vec![Value::number(8.0), Value::number(4.0), Value::number(3.0), Value::number(6.0)]));
        let Value::Number(m) = ev("MARGINOFERROR", vec![Arg::reference(data.clone()), n(0.95)]) else { panic!() };
        assert!((m - 3.528).abs() < 0.0005, "{m}");
        assert_eq!(ev("MARGINOFERROR", vec![Arg::reference(data.clone()), n(1.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("MARGINOFERROR", vec![Arg::reference(Value::number(1.0)), n(0.95)]), Value::Error(CellError::Div0));
        // AVERAGE.WEIGHTED: typed values are read as numbers; text in a range leaves its pair out.
        assert_eq!(ev("AVERAGE.WEIGHTED", vec![Arg::val(Value::text("10")), n(1.0), n(20.0), n(3.0)]), Value::number(17.5));
        assert_eq!(ev("AVERAGE.WEIGHTED", vec![Arg::val(Value::text("x")), n(1.0)]), Value::Error(CellError::Value));
        // ISBETWEEN lifts over a range; ARRAY_CONSTRAIN takes its range whole.
        let Value::Array(a) = ev("ISBETWEEN", vec![Arg::reference(data.clone()), n(4.0), n(6.0)]) else { panic!() };
        assert_eq!(a.data, vec![Value::Bool(false), Value::Bool(true), Value::Bool(false), Value::Bool(true)]);
        let Value::Array(a) = ev("ARRAY_CONSTRAIN", vec![Arg::reference(data), n(2.0), n(5.0)]) else { panic!() };
        assert_eq!(a.data, vec![Value::number(8.0), Value::number(4.0)]);
        // ISDATE of a value alone: text that reads as a date; a bare number isn't one.
        assert_eq!(ev("ISDATE", vec![Arg::val(Value::text("July 20 1969"))]), Value::Bool(true));
        assert_eq!(ev("ISDATE", vec![n(45_000.0)]), Value::Bool(false));
        assert_eq!(ev("EPOCHTODATE", vec![n(86_400.0)]), Value::number(25_570.0));
    }
}
