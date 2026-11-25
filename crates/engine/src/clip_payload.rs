//! A copy's cells as data inside its clipboard HTML, so pasting in another window of the app
//! (another browser tab, another workbook) brings formulas, formatting, notes, links, merged
//! cells and data validation, as Google Sheets does between its own files.
//!
//! Only the plain-text and HTML flavours survive the system clipboard in every browser (custom
//! types don't in Firefox), so the data rides on the copied `<table>` as one attribute. The
//! attribute is read only when the clipboard's plain text is exactly the copy's text, so HTML that
//! belongs to another copy is ignored. Anything on the clipboard can be written by another
//! program, so a payload is treated like a file someone else made: bounded in size before it is
//! decoded, its macros dropped, and its trust `FromFile`.

use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{CellStore, Sheet, Workbook};
use serde::{Deserialize, Serialize};

use crate::{Clipboard, Trust};

/// The attribute on the copied `<table>` that holds the payload (base64 of its JSON).
const ATTR: &str = "data-gridcraft-clip=\"";
const VERSION: u32 = 1;
/// The largest payload written or read, as JSON before base64: a copy bigger than this pastes
/// as text (values) in another window, as before.
pub(crate) const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;
/// Its base64 length, the most of the HTML that is ever scanned for the attribute's end.
const MAX_B64_BYTES: usize = MAX_JSON_BYTES.div_ceil(3) * 4;
/// The most cells a payload's block may cover (the HTML flavour stops at the same size).
const MAX_CELLS: u64 = 10_000;

#[derive(Serialize, Deserialize)]
struct Payload {
    v: u32,
    /// The copy's plain text: the payload counts only for a paste of this text.
    text: String,
    sheet: usize,
    range: RangeRef,
    wb: Workbook,
    /// Spilled values in the block (a sheet's spill map is derived state, not serialized).
    spill: Vec<(CellRef, Value)>,
}

/// `html` (the engine's copy HTML) with the copy's cells added as data, or unchanged when the
/// copy is too big to carry.
pub(crate) fn embed(html: String, clip: &Clipboard) -> String {
    match encode(clip) {
        Some(data) => html.replacen("<table", &format!("<table {ATTR}{data}\""), 1),
        None => html,
    }
}

fn encode(clip: &Clipboard) -> Option<String> {
    let r = clip.range;
    if r.count() > MAX_CELLS {
        return None;
    }
    let src = clip.wb.sheet(clip.sheet)?;
    let mut sh = Sheet::new(src.name.clone());
    let mut cells = CellStore::new();
    for (c, cell) in src.cells.iter_range(r) {
        cells.set(c, cell.clone());
    }
    sh.cells = cells;
    sh.cell_pictures = src.cell_pictures.iter().filter(|(c, _)| r.contains(**c)).map(|(c, p)| (*c, p.clone())).collect();
    sh.comments = src.comments.iter().filter(|(c, _)| r.contains(**c)).map(|(c, v)| (*c, v.clone())).collect();
    sh.hyperlinks = src.hyperlinks.iter().filter(|(c, _)| r.contains(**c)).map(|(c, v)| (*c, v.clone())).collect();
    sh.merges = src.merges.iter().filter(|m| r.contains_range(m)).copied().collect();
    sh.validations = src.validations.iter().filter(|dv| dv.ranges.iter().any(|x| x.intersects(&r))).cloned().collect();
    sh.cols = src.cols.range(r.start.col..=r.end.col).map(|(k, v)| (*k, *v)).collect();
    sh.rows = src.rows.range(r.start.row..=r.end.row).map(|(k, v)| (*k, *v)).collect();
    sh.default_col_width = src.default_col_width;
    sh.default_row_height = src.default_row_height;
    let spill = src.spill.range(r.start..=r.end).filter(|(c, _)| r.contains(**c)).map(|(c, v)| (*c, v.clone())).collect();
    // Other sheets keep their names (formulas refer to them by name) but nothing else.
    let sheets = clip
        .wb
        .sheets
        .iter()
        .enumerate()
        .map(|(i, s)| if i == clip.sheet { Arc::new(sh.clone()) } else { Arc::new(Sheet::new(s.name.clone())) })
        .collect();
    let mut wb = Workbook::new();
    wb.sheets = sheets;
    wb.names = clip.wb.names.clone();
    wb.styles = clip.wb.styles.clone();
    wb.theme = clip.wb.theme.clone();
    wb.date_system = clip.wb.date_system;
    let payload = Payload { v: VERSION, text: clip.text.clone(), sheet: clip.sheet, range: r, wb, spill };
    let json = serde_json::to_vec(&payload).ok()?;
    (json.len() <= MAX_JSON_BYTES).then(|| crate::io::base64_encode(&json))
}

/// The copy carried in `html`, when there is one and it was copied as `text`. Never the source
/// document's clipboard: it pastes as from another workbook (styles re-interned, a cut copies).
pub(crate) fn extract(html: &str, text: &str) -> Option<Clipboard> {
    let start = html.find(ATTR)? + ATTR.len();
    let rest = html.get(start..)?;
    let end = rest.bytes().take(MAX_B64_BYTES + 1).position(|b| b == b'"')?;
    let json = crate::io::base64_decode(rest.get(..end)?)?;
    if json.len() > MAX_JSON_BYTES {
        return None;
    }
    let p: Payload = serde_json::from_slice(&json).ok()?;
    if p.v != VERSION || !same_text(&p.text, text) {
        return None;
    }
    let r = p.range;
    if !r.start.is_valid() || !r.end.is_valid() || r.start.row > r.end.row || r.start.col > r.end.col || r.count() > MAX_CELLS {
        return None;
    }
    let mut wb = p.wb;
    // Clipboard data never brings code with it.
    wb.vba_project = None;
    wb.macros = Default::default();
    wb.macros_read.clear();
    wb.host_data = None;
    wb.styles.rebuild_index();
    for sh in &mut wb.sheets {
        Arc::make_mut(sh).cells.rebuild();
    }
    let sh = Arc::make_mut(wb.sheets.get_mut(p.sheet)?);
    sh.spill.extend(p.spill.into_iter().filter(|(c, _)| r.contains(*c)));
    Some(Clipboard {
        wb: Arc::new(wb),
        sheet: p.sheet,
        range: r,
        selected: r,
        cut: false,
        // No document has this uid: the paste re-interns styles as from another workbook.
        doc_uid: u64::MAX,
        text: p.text,
        areas: Vec::new(),
        trust: Trust::FromFile,
    })
}

/// Clipboard texts match as the engine's paste compares them: trailing line breaks and CRs aside.
pub(crate) fn same_text(a: &str, b: &str) -> bool {
    fn chars(t: &str) -> impl Iterator<Item = char> + '_ {
        t.trim_end_matches(['\r', '\n']).chars().filter(|c| *c != '\r')
    }
    chars(a).eq(chars(b))
}
