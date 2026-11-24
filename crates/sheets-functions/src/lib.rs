//! Google Sheets functions that GridCraft doesn't have (StarSheets): each takes plain values and
//! returns values, so it is tested here on its own. `gridcraft-functions` registers them (fork
//! commits "functions query (StarSheets 0023)" and "Google function batch 1 (StarSheets 0090)").

pub mod google;
#[cfg(test)]
mod google_tests;
pub mod query;

pub use query::{QueryError, query};
