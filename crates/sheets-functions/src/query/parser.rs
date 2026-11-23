//! Reads a query (Google's Visualization API Query Language) into a [`Query`].
//! Clauses, in the order the language requires: select, where, group by, pivot, order by,
//! limit, offset, label, format, options.

use super::QueryError;
use super::lexer::{Tok, Token, lex};

/// A value inside a query: what a cell holds, by query type.
#[derive(Clone, Debug, PartialEq)]
pub enum V {
    Null,
    Num(f64),
    Str(String),
    Bool(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agg {
    Avg,
    Count,
    Max,
    Min,
    Sum,
}

impl Agg {
    pub fn name(self) -> &'static str {
        match self {
            Agg::Avg => "avg",
            Agg::Count => "count",
            Agg::Max => "max",
            Agg::Min => "min",
            Agg::Sum => "sum",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
    StartsWith,
    EndsWith,
    Matches,
    Like,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Col(usize),
    Lit(V),
    Agg(Agg, Box<Expr>),
    /// A scalar function: year, month, day, upper, lower, dateDiff, now…
    Func(String, Vec<Expr>),
    Neg(Box<Expr>),
    /// `+ - * / %`.
    Arith(char, Box<Expr>, Box<Expr>),
    Cmp(Cmp, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    /// `is null` (false) / `is not null` (true).
    IsNull(Box<Expr>, bool),
}

impl Expr {
    pub fn has_agg(&self) -> bool {
        match self {
            Expr::Agg(..) => true,
            Expr::Col(_) | Expr::Lit(_) => false,
            Expr::Func(_, a) => a.iter().any(Expr::has_agg),
            Expr::Neg(e) | Expr::Not(e) | Expr::IsNull(e, _) => e.has_agg(),
            Expr::Arith(_, a, b) | Expr::Cmp(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) => a.has_agg() || b.has_agg(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Query {
    /// None = `select *` (or no select clause).
    pub select: Option<Vec<Expr>>,
    pub filter: Option<Expr>,
    pub group_by: Vec<Expr>,
    pub pivot: Vec<Expr>,
    /// (expression, descending).
    pub order_by: Vec<(Expr, bool)>,
    pub limit: Option<usize>,
    pub offset: usize,
    pub labels: Vec<(Expr, String)>,
    pub formats: Vec<(Expr, String)>,
}

/// Turns a column id into a column of the data: `A`…, `Col1`…, or a `backtick` id.
pub type Resolve<'a> = &'a dyn Fn(&str) -> Option<usize>;

const AGGS: [(&str, Agg); 5] = [("avg", Agg::Avg), ("count", Agg::Count), ("max", Agg::Max), ("min", Agg::Min), ("sum", Agg::Sum)];
/// Google's scalar functions (names lower-cased).
pub const FUNCS: [&str; 14] = ["year", "month", "day", "hour", "minute", "second", "millisecond", "quarter", "dayofweek", "now", "datediff", "todate", "upper", "lower"];
const CLAUSES: [&str; 10] = ["select", "where", "group", "pivot", "order", "limit", "offset", "label", "format", "options"];

struct Parser<'a> {
    toks: Vec<Token>,
    i: usize,
    resolve: Resolve<'a>,
}

pub fn parse(src: &str, resolve: Resolve<'_>) -> Result<Query, QueryError> {
    let mut p = Parser { toks: lex(src)?, i: 0, resolve };
    let mut q = Query::default();
    let mut last_clause = -1i32;
    while !p.at_end() {
        let word = match p.peek() {
            Tok::Word(w) => w.to_ascii_lowercase(),
            _ => return Err(p.err("expected a clause such as SELECT or WHERE")),
        };
        let Some(order) = CLAUSES.iter().position(|c| *c == word) else {
            return Err(p.err(&format!("unknown clause \"{word}\"")));
        };
        if order as i32 <= last_clause {
            return Err(p.err(&format!("\"{}\" is in the wrong place: the order is SELECT, WHERE, GROUP BY, PIVOT, ORDER BY, LIMIT, OFFSET, LABEL, FORMAT", word.to_uppercase())));
        }
        last_clause = order as i32;
        p.i += 1;
        match word.as_str() {
            "select" => {
                if p.peek() == &Tok::Sym("*") {
                    p.i += 1;
                } else {
                    q.select = Some(p.list()?);
                }
            }
            "where" => q.filter = Some(p.expr()?),
            "group" => {
                p.keyword("by")?;
                q.group_by = p.list()?;
            }
            "pivot" => q.pivot = p.list()?,
            "order" => {
                p.keyword("by")?;
                loop {
                    let e = p.expr()?;
                    let desc = match p.peek() {
                        Tok::Word(w) if w.eq_ignore_ascii_case("desc") => {
                            p.i += 1;
                            true
                        }
                        Tok::Word(w) if w.eq_ignore_ascii_case("asc") => {
                            p.i += 1;
                            false
                        }
                        _ => false,
                    };
                    q.order_by.push((e, desc));
                    if !p.comma() {
                        break;
                    }
                }
            }
            "limit" => q.limit = Some(p.count()?),
            "offset" => q.offset = p.count()?,
            "label" | "format" => loop {
                let e = p.expr()?;
                let Tok::Str(s) = p.next() else { return Err(p.err_prev("expected a quoted text after the column")) };
                if word == "label" { q.labels.push((e, s)) } else { q.formats.push((e, s)) }
                if !p.comma() {
                    break;
                }
            },
            // Options (no_values, no_format) change nothing here.
            _ => {
                while matches!(p.peek(), Tok::Word(w) if !CLAUSES.contains(&w.to_ascii_lowercase().as_str())) || p.peek() == &Tok::Sym(",") {
                    p.i += 1;
                }
            }
        }
    }
    Ok(q)
}

impl Parser<'_> {
    fn peek(&self) -> &Tok {
        &self.toks[self.i.min(self.toks.len() - 1)].tok
    }

    fn next(&mut self) -> Tok {
        let t = self.peek().clone();
        self.i = (self.i + 1).min(self.toks.len() - 1);
        t
    }

    fn at_end(&self) -> bool {
        self.peek() == &Tok::End
    }

    fn pos(&self) -> usize {
        self.toks[self.i.min(self.toks.len() - 1)].at
    }

    fn err(&self, msg: &str) -> QueryError {
        QueryError::at(self.pos(), msg.to_string())
    }

    fn err_prev(&self, msg: &str) -> QueryError {
        QueryError::at(self.toks[self.i.saturating_sub(1)].at, msg.to_string())
    }

    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Tok::Word(x) if x.eq_ignore_ascii_case(w))
    }

    fn keyword(&mut self, w: &str) -> Result<(), QueryError> {
        if self.is_word(w) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err(&format!("expected \"{}\"", w.to_uppercase())))
        }
    }

    fn comma(&mut self) -> bool {
        if self.peek() == &Tok::Sym(",") {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn list(&mut self) -> Result<Vec<Expr>, QueryError> {
        let mut v = vec![self.expr()?];
        while self.comma() {
            v.push(self.expr()?);
        }
        Ok(v)
    }

    fn count(&mut self) -> Result<usize, QueryError> {
        match self.next() {
            Tok::Num(n) if n >= 0.0 && n.fract() == 0.0 => Ok(n as usize),
            _ => Err(self.err_prev("expected a whole number")),
        }
    }

    pub fn expr(&mut self) -> Result<Expr, QueryError> {
        let mut l = self.and()?;
        while self.is_word("or") {
            self.i += 1;
            let r = self.and()?;
            l = Expr::Or(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn and(&mut self) -> Result<Expr, QueryError> {
        let mut l = self.not()?;
        while self.is_word("and") {
            self.i += 1;
            let r = self.not()?;
            l = Expr::And(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn not(&mut self) -> Result<Expr, QueryError> {
        if self.is_word("not") {
            self.i += 1;
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Expr, QueryError> {
        let l = self.additive()?;
        let op = match self.peek().clone() {
            Tok::Sym("=") => Some(Cmp::Eq),
            Tok::Sym("!=") | Tok::Sym("<>") => Some(Cmp::Ne),
            Tok::Sym("<") => Some(Cmp::Lt),
            Tok::Sym("<=") => Some(Cmp::Le),
            Tok::Sym(">") => Some(Cmp::Gt),
            Tok::Sym(">=") => Some(Cmp::Ge),
            Tok::Word(w) => match w.to_ascii_lowercase().as_str() {
                "contains" => Some(Cmp::Contains),
                "matches" => Some(Cmp::Matches),
                "like" => Some(Cmp::Like),
                "starts" => {
                    self.i += 1;
                    self.keyword("with")?;
                    self.i -= 1;
                    Some(Cmp::StartsWith)
                }
                "ends" => {
                    self.i += 1;
                    self.keyword("with")?;
                    self.i -= 1;
                    Some(Cmp::EndsWith)
                }
                "is" => {
                    self.i += 1;
                    let negate = self.is_word("not");
                    if negate {
                        self.i += 1;
                    }
                    self.keyword("null")?;
                    return Ok(Expr::IsNull(Box::new(l), negate));
                }
                _ => None,
            },
            _ => None,
        };
        let Some(op) = op else { return Ok(l) };
        self.i += 1;
        let r = self.additive()?;
        Ok(Expr::Cmp(op, Box::new(l), Box::new(r)))
    }

    fn additive(&mut self) -> Result<Expr, QueryError> {
        let mut l = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Tok::Sym("+") => '+',
                Tok::Sym("-") => '-',
                _ => return Ok(l),
            };
            self.i += 1;
            let r = self.multiplicative()?;
            l = Expr::Arith(op, Box::new(l), Box::new(r));
        }
    }

    fn multiplicative(&mut self) -> Result<Expr, QueryError> {
        let mut l = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Sym("*") => '*',
                Tok::Sym("/") => '/',
                Tok::Sym("%") => '%',
                _ => return Ok(l),
            };
            self.i += 1;
            let r = self.unary()?;
            l = Expr::Arith(op, Box::new(l), Box::new(r));
        }
    }

    fn unary(&mut self) -> Result<Expr, QueryError> {
        if self.peek() == &Tok::Sym("-") {
            self.i += 1;
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, QueryError> {
        let at = self.pos();
        match self.next() {
            Tok::Num(n) => Ok(Expr::Lit(V::Num(n))),
            Tok::Str(s) => Ok(Expr::Lit(V::Str(s))),
            Tok::Sym("(") => {
                let e = self.expr()?;
                match self.next() {
                    Tok::Sym(")") => Ok(e),
                    _ => Err(self.err_prev("expected \")\"")),
                }
            }
            Tok::Quoted(id) => self.column(&id, at),
            Tok::Word(w) => {
                let lower = w.to_ascii_lowercase();
                match lower.as_str() {
                    "true" => return Ok(Expr::Lit(V::Bool(true))),
                    "false" => return Ok(Expr::Lit(V::Bool(false))),
                    "date" | "datetime" | "timeofday" if matches!(self.peek(), Tok::Str(_)) => {
                        let Tok::Str(s) = self.next() else { unreachable!() };
                        return super::dates::literal(&lower, &s).map(|n| Expr::Lit(V::Num(n))).ok_or_else(|| QueryError::at(at, format!("bad {lower} \"{s}\"")));
                    }
                    _ => {}
                }
                if self.peek() == &Tok::Sym("(") {
                    self.i += 1;
                    let mut args = Vec::new();
                    if self.peek() != &Tok::Sym(")") {
                        args = self.list()?;
                    }
                    if self.next() != Tok::Sym(")") {
                        return Err(self.err_prev("expected \")\""));
                    }
                    if let Some((_, agg)) = AGGS.iter().find(|(n, _)| *n == lower) {
                        let [arg] = <[Expr; 1]>::try_from(args).map_err(|_| QueryError::at(at, format!("{lower}() takes one column")))?;
                        return Ok(Expr::Agg(*agg, Box::new(arg)));
                    }
                    if FUNCS.contains(&lower.as_str()) {
                        return Ok(Expr::Func(lower, args));
                    }
                    return Err(QueryError::at(at, format!("unknown function \"{w}\"")));
                }
                self.column(&w, at)
            }
            Tok::End => Err(QueryError::at(at, "the query ends too soon".into())),
            t => Err(QueryError::at(at, format!("unexpected {}", describe(&t)))),
        }
    }

    fn column(&self, id: &str, at: usize) -> Result<Expr, QueryError> {
        (self.resolve)(id).map(Expr::Col).ok_or_else(|| QueryError::at(at, format!("no column \"{id}\" in the data")))
    }
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Word(w) | Tok::Quoted(w) => format!("\"{w}\""),
        Tok::Num(n) => n.to_string(),
        Tok::Str(s) => format!("'{s}'"),
        Tok::Sym(s) => format!("\"{s}\""),
        Tok::End => "the end".into(),
    }
}
