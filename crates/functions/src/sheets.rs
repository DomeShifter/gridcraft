//! Google Sheets functions that Excel doesn't have, batch 1 (StarSheets 0090): SPLIT, JOIN,
//! REGEXMATCH, COUNTUNIQUE, FLATTEN, ISEMAIL, ISURL, TO_TEXT and the operator functions. The
//! work is done in the StarSheets crate `sheets-functions` (`google.rs`, tested there); this
//! registers them. Formulas using them are saved to .xlsx in Google's export wrapper
//! (`__xludf.DUMMYFUNCTION`, xlsx `sheet_write.rs` GOOGLE_ONLY), as QUERY is.

use gridcraft_core::Value;
use sheets_functions::google::{self, Op};

use crate::util::{A, R, S, arg, opt_bool, scalar};
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

/// SPLIT takes its text whole (a column splits row by row); its other arguments lift.
const SPLIT_ARGS: &[bool] = &[false, true];

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
}
