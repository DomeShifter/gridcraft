//! Google Sheets' QUERY(data, query, [headers]): the Visualization API Query Language over a
//! block of cells. Reading the query is in `parser`, running it in `exec`; the tests compare
//! against answers recorded from Google Sheets itself (tests.rs).

mod dates;
mod exec;
mod lexer;
mod parser;
#[cfg(test)]
mod tests;

use gridcraft_core::Array;

/// A query that can't run: where in the query text, and why (Google's wording where it has one).
#[derive(Clone, Debug, PartialEq)]
pub struct QueryError {
    pub at: usize,
    pub msg: String,
}

impl QueryError {
    pub(crate) fn at(at: usize, msg: String) -> QueryError {
        QueryError { at, msg }
    }
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Unable to parse query string for Function QUERY parameter 2: {} (at character {})", self.msg, self.at + 1)
    }
}

/// Runs `query` over `data`.
/// - `headers`: header rows at the top of `data`; None (or a negative number) guesses, as Sheets does.
/// - `first_col`: the sheet column (0 = A) of the data's first column when it came from a range,
///   so the query names columns by their letters (`select C`); None for an array (`Col1`, `Col2`…).
/// - `now`: the current date-time serial, for now().
pub fn query(data: &Array, query: &str, headers: Option<i64>, first_col: Option<u32>, now: f64) -> Result<Array, QueryError> {
    exec::run(data, query, headers, first_col, now)
}
