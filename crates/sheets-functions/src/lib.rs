//! Google Sheets functions that GridCraft doesn't have (StarSheets): each takes plain values and
//! returns values, so it is tested here on its own. `gridcraft-functions` registers them (fork
//! commit "functions query (StarSheets 0023)").

pub mod query;

pub use query::{QueryError, query};
