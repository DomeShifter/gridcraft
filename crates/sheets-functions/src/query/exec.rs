//! Runs a parsed query: column types, WHERE, GROUP BY with aggregates, PIVOT, ORDER BY,
//! OFFSET/LIMIT, then the header row (labels) and FORMAT, in Google's order.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use gridcraft_core::{Array, Value};

use super::QueryError;
use super::dates;
use super::parser::{Agg, Cmp, Expr, V, parse};

pub fn run(data: &Array, src: &str, headers: Option<i64>, first_col: Option<u32>, now: f64) -> Result<Array, QueryError> {
    let cols = data.cols;
    // Rows past the last one with anything in them don't count (a whole-column range).
    let mut rows = data.rows;
    while rows > 0 && (0..cols).all(|c| data.get(rows - 1, c).is_none_or(is_blank)) {
        rows -= 1;
    }
    let cell = |r: usize, c: usize| data.get(r, c).cloned().unwrap_or(Value::Empty);
    let header_rows = match headers {
        Some(h) if h >= 0 => (h as usize).min(rows),
        _ => guess_headers(data, rows),
    };
    // A column's label: its header cells, joined by spaces.
    let labels: Vec<String> = (0..cols)
        .map(|c| (0..header_rows).map(|r| cell(r, c).display()).filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join(" "))
        .collect();
    // Values, with each column given one type (the most common): other values become null, as
    // in Sheets.
    let mut table: Vec<Vec<V>> = (header_rows..rows).map(|r| (0..cols).map(|c| to_v(&cell(r, c))).collect()).collect();
    for c in 0..cols {
        let (mut n, mut s, mut b) = (0usize, 0usize, 0usize);
        for row in &table {
            match row[c] {
                V::Num(_) => n += 1,
                V::Str(_) => s += 1,
                V::Bool(_) => b += 1,
                V::Null => {}
            }
        }
        let keep = if n >= s && n >= b { 0 } else if s >= b { 1 } else { 2 };
        for row in &mut table {
            let kind = match row[c] {
                V::Num(_) => 0,
                V::Str(_) => 1,
                V::Bool(_) => 2,
                V::Null => continue,
            };
            if kind != keep {
                row[c] = V::Null;
            }
        }
    }
    let resolve = |id: &str| -> Option<usize> {
        let up = id.to_ascii_uppercase();
        if let Some(n) = up.strip_prefix("COL").and_then(|n| n.parse::<usize>().ok()) {
            return (n >= 1 && n <= cols).then(|| n - 1);
        }
        if !up.is_empty() && up.len() <= 3 && up.chars().all(|c| c.is_ascii_uppercase()) {
            let letters = up.bytes().fold(0u32, |acc, b| acc * 26 + u32::from(b - b'A' + 1)) - 1;
            let c = letters.checked_sub(first_col.unwrap_or(0))? as usize;
            return (c < cols).then_some(c);
        }
        None
    };
    let q = parse(src, &resolve)?;
    let cx = Cx { table: &table, labels: &labels, now };

    // WHERE.
    let mut kept: Vec<usize> = Vec::new();
    if let Some(f) = &q.filter {
        if f.has_agg() {
            return Err(QueryError::at(0, "WHERE can't use sum, avg, count, min or max".into()));
        }
        for i in 0..table.len() {
            if truthy(&cx.eval(f, &Scope::Row(i))?) {
                kept.push(i);
            }
        }
    } else {
        kept = (0..table.len()).collect();
    }

    let select: Vec<Expr> = q.select.clone().unwrap_or_else(|| (0..cols).map(Expr::Col).collect());
    let grouped = !q.group_by.is_empty() || !q.pivot.is_empty() || select.iter().any(Expr::has_agg);

    // Each output row is a scope (a data row, or a group of rows) with its values.
    let mut out_cols: Vec<OutCol> = Vec::new();
    let mut scopes: Vec<Scope> = Vec::new();
    if grouped {
        for e in &select {
            if !e.has_agg() && !q.group_by.contains(e) && !q.pivot.contains(e) {
                return Err(QueryError::at(0, format!("{} must be in GROUP BY, or inside sum, avg, count, min or max", cx.label(e))));
            }
        }
        // Groups, ordered by their keys (as Sheets lists them).
        let mut groups: BTreeMap<Vec<Key>, Vec<usize>> = BTreeMap::new();
        for &i in &kept {
            let key = q.group_by.iter().map(|g| cx.eval(g, &Scope::Row(i)).map(Key)).collect::<Result<Vec<_>, _>>()?;
            groups.entry(key).or_default().push(i);
        }
        if groups.is_empty() && q.group_by.is_empty() {
            groups.insert(Vec::new(), Vec::new());
        }
        // Pivot values, each becoming a set of columns.
        let mut pivots: Vec<Vec<V>> = Vec::new();
        if !q.pivot.is_empty() {
            let mut seen: BTreeMap<Vec<Key>, ()> = BTreeMap::new();
            for &i in &kept {
                let k = q.pivot.iter().map(|p| cx.eval(p, &Scope::Row(i)).map(Key)).collect::<Result<Vec<_>, _>>()?;
                seen.insert(k, ());
            }
            pivots = seen.into_keys().map(|k| k.into_iter().map(|Key(v)| v).collect()).collect();
        }
        // With one total, Sheets names each pivot column by its value alone ("Eng"); with
        // several, by value and total ("Eng sum Salary").
        let totals = select.iter().filter(|e| e.has_agg()).count();
        for e in &select {
            if q.pivot.is_empty() || !e.has_agg() {
                out_cols.push(OutCol { expr: e.clone(), pivot: None, label: cx.label(e) });
            } else {
                for p in &pivots {
                    let name = p.iter().map(v_text).collect::<Vec<_>>().join(", ");
                    let label = if totals == 1 { name } else { format!("{name} {}", cx.label(e)) };
                    out_cols.push(OutCol { expr: e.clone(), pivot: Some(p.clone()), label });
                }
            }
        }
        for rows in groups.into_values() {
            scopes.push(Scope::Group(rows));
        }
    } else {
        out_cols = select.iter().map(|e| OutCol { expr: e.clone(), pivot: None, label: cx.label(e) }).collect();
        scopes = kept.into_iter().map(Scope::Row).collect();
    }

    // ORDER BY (stable, so rows keep their order otherwise).
    if !q.order_by.is_empty() {
        let mut keyed = Vec::with_capacity(scopes.len());
        for s in scopes {
            let keys = q.order_by.iter().map(|(e, _)| cx.eval(e, &s)).collect::<Result<Vec<_>, _>>()?;
            keyed.push((keys, s));
        }
        keyed.sort_by(|(a, _), (b, _)| {
            for (i, (_, desc)) in q.order_by.iter().enumerate() {
                let o = compare(&a[i], &b[i]);
                let o = if *desc { o.reverse() } else { o };
                if o != Ordering::Equal {
                    return o;
                }
            }
            Ordering::Equal
        });
        scopes = keyed.into_iter().map(|(_, s)| s).collect();
    }
    let scopes: Vec<Scope> = scopes.into_iter().skip(q.offset).take(q.limit.unwrap_or(usize::MAX)).collect();

    // LABEL and FORMAT go by the select expression they name.
    for (e, text) in &q.labels {
        for c in out_cols.iter_mut().filter(|c| c.expr == *e) {
            c.label = match &c.pivot {
                Some(p) if !text.is_empty() => format!("{} {text}", p.iter().map(v_text).collect::<Vec<_>>().join(", ")),
                _ => text.clone(),
            };
        }
    }
    let mut cells: Vec<Value> = Vec::new();
    let header = out_cols.iter().any(|c| !c.label.is_empty());
    if header {
        cells.extend(out_cols.iter().map(|c| if c.label.is_empty() { Value::Empty } else { Value::Text(c.label.as_str().into()) }));
    }
    for s in &scopes {
        for c in &out_cols {
            let v = match (&c.pivot, s) {
                (Some(p), Scope::Group(rows)) => {
                    let inside: Vec<usize> = rows
                        .iter()
                        .copied()
                        .filter(|&i| q.pivot.iter().zip(p).all(|(pe, pv)| cx.eval(pe, &Scope::Row(i)).is_ok_and(|v| Key(v) == Key(pv.clone()))))
                        .collect();
                    // A pivot cell no row falls into is blank (not a count of 0), as in Sheets.
                    if inside.is_empty() { V::Null } else { cx.eval(&c.expr, &Scope::Group(inside))? }
                }
                _ => cx.eval(&c.expr, s)?,
            };
            let format = q.formats.iter().find(|(e, _)| *e == c.expr).map(|(_, f)| f.as_str());
            cells.push(output(v, format));
        }
    }
    let n_rows = scopes.len() + usize::from(header);
    if n_rows == 0 || out_cols.is_empty() {
        return Ok(Array::new(1, 1, vec![Value::Error(gridcraft_core::CellError::NA)]).expect("one cell"));
    }
    Array::new(n_rows, out_cols.len(), cells).ok_or_else(|| QueryError::at(0, "the result is too large".into()))
}

struct OutCol {
    expr: Expr,
    pivot: Option<Vec<V>>,
    label: String,
}

enum Scope {
    Row(usize),
    Group(Vec<usize>),
}

struct Cx<'a> {
    table: &'a [Vec<V>],
    labels: &'a [String],
    now: f64,
}

impl Cx<'_> {
    /// An expression's column label, as Sheets writes it: the header, `sum Score`, `year(Date)`.
    fn label(&self, e: &Expr) -> String {
        match e {
            Expr::Col(c) => self.labels[*c].clone(),
            Expr::Lit(v) => v_text(v),
            Expr::Agg(a, x) => format!("{} {}", a.name(), self.label(x)),
            Expr::Func(n, args) => format!("{}({})", func_name(n), args.iter().map(|a| self.label(a)).collect::<Vec<_>>().join(", ")),
            Expr::Neg(x) => format!("-{}", self.label(x)),
            Expr::Arith(op, a, b) => format!("{}{op}{}", self.label(a), self.label(b)),
            _ => String::new(),
        }
    }

    fn eval(&self, e: &Expr, s: &Scope) -> Result<V, QueryError> {
        Ok(match e {
            Expr::Col(c) => match s {
                Scope::Row(i) => self.table[*i][*c].clone(),
                Scope::Group(rows) => rows.first().map_or(V::Null, |&i| self.table[i][*c].clone()),
            },
            Expr::Lit(v) => v.clone(),
            Expr::Agg(a, x) => {
                let Scope::Group(rows) = s else {
                    return Err(QueryError::at(0, format!("{}() needs GROUP BY or only totals in SELECT", a.name())));
                };
                let vals = rows.iter().map(|&i| self.eval(x, &Scope::Row(i))).collect::<Result<Vec<_>, _>>()?;
                aggregate(*a, vals)
            }
            Expr::Func(n, args) => {
                let vals = args.iter().map(|a| self.eval(a, s)).collect::<Result<Vec<_>, _>>()?;
                func(n, &vals, self.now)
            }
            Expr::Neg(x) => match self.eval(x, s)? {
                V::Num(n) => V::Num(-n),
                _ => V::Null,
            },
            Expr::Arith(op, a, b) => match (self.eval(a, s)?, self.eval(b, s)?) {
                (V::Num(x), V::Num(y)) => match op {
                    '+' => V::Num(x + y),
                    '-' => V::Num(x - y),
                    '*' => V::Num(x * y),
                    '/' if y != 0.0 => V::Num(x / y),
                    '%' if y != 0.0 => V::Num(x % y),
                    _ => V::Null,
                },
                _ => V::Null,
            },
            Expr::Cmp(op, a, b) => V::Bool(compare_op(*op, &self.eval(a, s)?, &self.eval(b, s)?)),
            Expr::And(a, b) => V::Bool(truthy(&self.eval(a, s)?) && truthy(&self.eval(b, s)?)),
            Expr::Or(a, b) => V::Bool(truthy(&self.eval(a, s)?) || truthy(&self.eval(b, s)?)),
            Expr::Not(a) => V::Bool(!truthy(&self.eval(a, s)?)),
            Expr::IsNull(a, negate) => V::Bool((self.eval(a, s)? == V::Null) != *negate),
        })
    }
}

/// A function's name in a label: Google writes them in lower case (`dayofweek(Start)`).
fn func_name(n: &str) -> &str {
    n
}

fn aggregate(a: Agg, vals: Vec<V>) -> V {
    let present: Vec<V> = vals.into_iter().filter(|v| *v != V::Null).collect();
    match a {
        Agg::Count => V::Num(present.len() as f64),
        Agg::Sum | Agg::Avg => {
            let nums: Vec<f64> = present.iter().filter_map(|v| if let V::Num(n) = v { Some(*n) } else { None }).collect();
            if nums.is_empty() {
                return V::Null;
            }
            let total: f64 = nums.iter().sum();
            V::Num(if a == Agg::Sum { total } else { total / nums.len() as f64 })
        }
        Agg::Min => present.into_iter().min_by(compare).unwrap_or(V::Null),
        Agg::Max => present.into_iter().max_by(compare).unwrap_or(V::Null),
    }
}

fn func(n: &str, v: &[V], now: f64) -> V {
    let num = |i: usize| if let Some(V::Num(x)) = v.get(i) { Some(*x) } else { None };
    let text = |i: usize| if let Some(V::Str(x)) = v.get(i) { Some(x.clone()) } else { None };
    match n {
        "now" => V::Num(now),
        "upper" => text(0).map_or(V::Null, |s| V::Str(s.to_uppercase())),
        "lower" => text(0).map_or(V::Null, |s| V::Str(s.to_lowercase())),
        "todate" => num(0).map_or(V::Null, |x| V::Num(x.floor())),
        "datediff" => match (num(0), num(1)) {
            (Some(a), Some(b)) => V::Num(a.floor() - b.floor()),
            _ => V::Null,
        },
        _ => {
            let Some(x) = num(0) else { return V::Null };
            let (y, m, d) = dates::parts(x);
            let (h, mi, s, ms) = dates::clock(x);
            V::Num(match n {
                "year" => f64::from(y),
                // Google counts months from 0 (January = 0).
                "month" => f64::from(m) - 1.0,
                "day" => f64::from(d),
                "hour" => f64::from(h),
                "minute" => f64::from(mi),
                "second" => f64::from(s),
                "millisecond" => f64::from(ms),
                "quarter" => f64::from((m - 1) / 3 + 1),
                // 1 = Sunday … 7 = Saturday (serial 1 = Sunday 31 Dec 1899).
                "dayofweek" => ((x.floor() as i64 - 1).rem_euclid(7) + 1) as f64,
                _ => return V::Null,
            })
        }
    }
}

fn compare_op(op: Cmp, a: &V, b: &V) -> bool {
    if *a == V::Null || *b == V::Null {
        return false;
    }
    let strs = match (a, b) {
        (V::Str(x), V::Str(y)) => Some((x.as_str(), y.as_str())),
        _ => None,
    };
    match op {
        Cmp::Eq => same_type(a, b) && compare(a, b) == Ordering::Equal,
        Cmp::Ne => !same_type(a, b) || compare(a, b) != Ordering::Equal,
        Cmp::Lt => same_type(a, b) && compare(a, b) == Ordering::Less,
        Cmp::Le => same_type(a, b) && compare(a, b) != Ordering::Greater,
        Cmp::Gt => same_type(a, b) && compare(a, b) == Ordering::Greater,
        Cmp::Ge => same_type(a, b) && compare(a, b) != Ordering::Less,
        Cmp::Contains => strs.is_some_and(|(x, y)| x.contains(y)),
        Cmp::StartsWith => strs.is_some_and(|(x, y)| x.starts_with(y)),
        Cmp::EndsWith => strs.is_some_and(|(x, y)| x.ends_with(y)),
        Cmp::Matches => strs.is_some_and(|(x, y)| regex::Regex::new(&format!("^(?:{y})$")).is_ok_and(|r| r.is_match(x))),
        Cmp::Like => strs.is_some_and(|(x, y)| {
            let mut pat = String::from("^");
            for ch in y.chars() {
                match ch {
                    '%' => pat.push_str(".*"),
                    '_' => pat.push('.'),
                    c => pat.push_str(&regex::escape(&c.to_string())),
                }
            }
            pat.push('$');
            regex::Regex::new(&format!("(?s){pat}")).is_ok_and(|r| r.is_match(x))
        }),
    }
}

fn same_type(a: &V, b: &V) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// Sort order: empty first, then numbers, text, true/false.
fn compare(a: &V, b: &V) -> Ordering {
    fn rank(v: &V) -> u8 {
        match v {
            V::Null => 0,
            V::Num(_) => 1,
            V::Str(_) => 2,
            V::Bool(_) => 3,
        }
    }
    match (a, b) {
        (V::Num(x), V::Num(y)) => x.total_cmp(y),
        (V::Str(x), V::Str(y)) => x.cmp(y),
        (V::Bool(x), V::Bool(y)) => x.cmp(y),
        _ => rank(a).cmp(&rank(b)),
    }
}

/// A value as a map key (groups, pivot columns), ordered like ORDER BY.
#[derive(Clone, Debug)]
struct Key(V);

impl PartialEq for Key {
    fn eq(&self, o: &Key) -> bool {
        compare(&self.0, &o.0) == Ordering::Equal
    }
}
impl Eq for Key {}
impl PartialOrd for Key {
    fn partial_cmp(&self, o: &Key) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Key {
    fn cmp(&self, o: &Key) -> Ordering {
        compare(&self.0, &o.0)
    }
}

fn truthy(v: &V) -> bool {
    match v {
        V::Bool(b) => *b,
        V::Num(n) => *n != 0.0,
        _ => false,
    }
}

fn is_blank(v: &Value) -> bool {
    matches!(v, Value::Empty) || matches!(v, Value::Text(t) if t.is_empty())
}

fn to_v(v: &Value) -> V {
    match v {
        Value::Number(n) => V::Num(*n),
        Value::Text(t) if t.is_empty() => V::Null,
        Value::Text(t) => V::Str(t.to_string()),
        Value::Bool(b) => V::Bool(*b),
        _ => V::Null,
    }
}

fn v_text(v: &V) -> String {
    match v {
        V::Null => String::new(),
        V::Num(n) => Value::number(*n).display(),
        V::Str(s) => s.clone(),
        V::Bool(b) => if *b { "TRUE".into() } else { "FALSE".into() },
    }
}

fn output(v: V, format: Option<&str>) -> Value {
    let value = match v {
        V::Null => Value::Empty,
        V::Num(n) => Value::number(n),
        V::Str(s) => Value::Text(s.into()),
        V::Bool(b) => Value::Bool(b),
    };
    match format {
        Some(f) if value != Value::Empty => gridcraft_numfmt::text_function(&value, f, gridcraft_core::date::DateSystem::default()).map_or(value, |s| Value::Text(s.into())),
        _ => value,
    }
}

/// Sheets' guess at the header rows: 1 when the first row is all text and some column below it
/// holds numbers, dates or true/false; else 0.
fn guess_headers(data: &Array, rows: usize) -> usize {
    if rows < 2 {
        return 0;
    }
    let first_all_text = (0..data.cols).all(|c| matches!(data.get(0, c), Some(Value::Text(t)) if !t.is_empty()) || data.get(0, c).is_none_or(is_blank));
    let any_text = (0..data.cols).any(|c| matches!(data.get(0, c), Some(Value::Text(t)) if !t.is_empty()));
    let below_not_text = (0..data.cols).any(|c| (1..rows).any(|r| matches!(data.get(r, c), Some(Value::Number(_) | Value::Bool(_)))));
    usize::from(first_all_text && any_text && below_not_text)
}
