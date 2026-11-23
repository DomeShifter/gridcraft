//! Splits a query string into tokens. Keywords and column names are case-insensitive; string
//! literals use single or double quotes; `backticks` quote a column id.

use super::QueryError;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// A word: a keyword, a column id (`A`, `Col1`) or a function name.
    Word(String),
    /// A `backtick`-quoted column id.
    Quoted(String),
    Num(f64),
    Str(String),
    /// `=`, `!=`, `<>`, `<`, `<=`, `>`, `>=`, `+`, `-`, `*`, `/`, `%`, `,`, `(`, `)`.
    Sym(&'static str),
    End,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    /// Where it starts, for error messages.
    pub at: usize,
}

pub fn lex(src: &str) -> Result<Vec<Token>, QueryError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let at = i;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text: String = chars[start..i].iter().collect();
            let n = text.parse::<f64>().map_err(|_| QueryError::at(at, format!("bad number {text}")))?;
            out.push(Token { tok: Tok::Num(n), at });
            continue;
        }
        if c == '\'' || c == '"' || c == '`' {
            let quote = c;
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != quote {
                i += 1;
            }
            if i >= chars.len() {
                return Err(QueryError::at(at, "a quote is never closed".into()));
            }
            let text: String = chars[start..i].iter().collect();
            i += 1;
            out.push(Token { tok: if quote == '`' { Tok::Quoted(text) } else { Tok::Str(text) }, at });
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Token { tok: Tok::Word(chars[start..i].iter().collect()), at });
            continue;
        }
        let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
        let sym = match two.as_str() {
            "!=" => Some("!="),
            "<>" => Some("<>"),
            "<=" => Some("<="),
            ">=" => Some(">="),
            _ => None,
        };
        if let Some(s) = sym {
            out.push(Token { tok: Tok::Sym(s), at });
            i += 2;
            continue;
        }
        let one = match c {
            '=' => "=",
            '<' => "<",
            '>' => ">",
            '+' => "+",
            '-' => "-",
            '*' => "*",
            '/' => "/",
            '%' => "%",
            ',' => ",",
            '(' => "(",
            ')' => ")",
            _ => return Err(QueryError::at(at, format!("unexpected \"{c}\""))),
        };
        out.push(Token { tok: Tok::Sym(one), at });
        i += 1;
    }
    out.push(Token { tok: Tok::End, at: chars.len() });
    Ok(out)
}
