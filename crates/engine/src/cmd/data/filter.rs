use std::collections::BTreeMap;

use gridcraft_model::{Color, FilterView, ViewRows};

use super::sort::sort_key;
use super::{
    Arc, AutoFilter, CellRef, EngineError, FilterCriterion, Json, LineInfo, Ordering, RangeRef, Result, Session, Sheet, Value, Workbook, bad,
    bool_param, col_param, current_region, data_target, edit, json, str_param, target_sheet,
};

// ---------------------------------------------------------------- filter

pub(super) fn toggle_filter(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, r) = data_target(s, p, "data.filter")?;
    crate::cmd::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    // The active cell belongs to the active sheet: on another sheet the table is the one at `range`.
    let at = if sheet == s.doc()?.wb.active_sheet() { s.doc()?.selection.active } else { r.start };
    let has = s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.autofilter.is_some());
    let in_table = s.doc()?.wb.sheet(sheet).and_then(|sh| sh.table_at(at).map(|t| t.name.clone()));
    if let Some(name) = in_table {
        return edit(s, |cx| {
            let sh = cx.sheet_mut(sheet)?;
            if let Some(t) = sh.tables.iter_mut().find(|t| t.name == name) {
                t.filter_button = !t.filter_button;
                if !t.filter_button {
                    t.filter.clear();
                }
            }
            Ok(Json::Null)
        });
    }
    if has {
        return edit(s, |cx| {
            let sh = cx.sheet_mut(sheet)?;
            // Reveal only rows the filter hid; rows hidden by hand or by a collapsed group stay hidden.
            let filtered: Vec<u32> = sh.rows.keys().copied().filter(|r| sh.is_row_filtered(*r)).collect();
            sh.autofilter = None;
            for r in filtered {
                if let Some(info) = sh.rows.get_mut(&r) {
                    info.hidden = false;
                }
            }
            cx.structural = true;
            Ok(json!({"filter": false}))
        });
    }
    install_filter(s, p)
}

/// The range a new filter (or filter view) covers: `p.range`, or the data range of the selection.
fn new_filter_range(s: &Session, p: &Json, cmd: &str) -> Result<(usize, RangeRef)> {
    let (sheet, mut r) = data_target(s, p, cmd)?;
    // A selected header row filters the data below it (#201).
    if p.get("range").is_none() && r.height() == 1 {
        let d = s.doc()?;
        let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
        let below = current_region(sh, r.start).end.row;
        if below > r.end.row {
            r = RangeRef::new(r.start, CellRef::new(below, r.end.col));
        }
    }
    Ok((sheet, r))
}

/// Turn a sheet AutoFilter on over `p.range`, or over the data range of the selection.
fn install_filter(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, r) = new_filter_range(s, p, "data.filter")?;
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.autofilter = Some(AutoFilter { range: r, criteria: vec![] });
        Ok(json!({"filter": true, "range": r.a1()}))
    })
}

/// The filter view `p.view` names, if it names one: its id and range.
fn view_param(s: &Session, sheet: usize, p: &Json, cmd: &str) -> Result<Option<(u32, RangeRef)>> {
    let Some(v) = p.get("view").filter(|v| !v.is_null()) else { return Ok(None) };
    let id = v.as_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad(cmd, "`view` must be a filter view's id"))?;
    let sh = s.doc()?.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    sh.filter_views.iter().find(|v| v.id == id).map(|v| Some((id, v.range))).ok_or_else(|| bad(cmd, format!("there is no filter view {id}")))
}

/// A colour parameter: "#RRGGBB" or a colour as GridCraft writes it in JSON (`{"Theme": [4, 0]}`).
fn color_param(v: &Json, cmd: &str) -> Result<Color> {
    match v {
        Json::String(t) => Color::from_hex(t).ok_or_else(|| bad(cmd, format!("`{t}` is not a colour"))),
        other => serde_json::from_value(other.clone()).map_err(|_| bad(cmd, "not a colour")),
    }
}

pub(super) fn filter_by(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = s.doc()?.wb.active_sheet();
    // A filter view hides rows only where it is open, so a protected sheet still allows it.
    let view = view_param(s, sheet, p, "data.filterBy")?;
    if view.is_none() {
        crate::cmd::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    }
    let Some(sh) = s.doc()?.wb.sheet(sheet) else { return Err(EngineError::NoDocument) };
    let current = sh.autofilter.as_ref().map(|a| a.range);
    let range = match (view, current) {
        (Some((_, r)), _) => r,
        (None, Some(r)) => r,
        (None, None) => {
            install_filter(s, &json!({}))?;
            s.doc()?.wb.sheet(sheet).and_then(|sh| sh.autofilter.as_ref().map(|a| a.range)).ok_or_else(|| EngineError::Other("no filter".into()))?
        }
    };
    let af = AutoFilter { range, criteria: vec![] };
    let col = match col_param(p.get("column"), af.range.start.col) {
        Some(c) => c,
        // A number too large to be a column is outside the filter range, not a missing parameter.
        None if p.get("column").is_some_and(|v| v.as_u64().is_some()) => return Err(bad("data.filterBy", "column outside the filter range")),
        None => return Err(bad("data.filterBy", "missing `column`")),
    };
    if col < af.range.start.col || col > af.range.end.col {
        return Err(bad("data.filterBy", "column outside the filter range"));
    }
    let off = col - af.range.start.col;
    let crit = if bool_param(p, "clear").unwrap_or(false) {
        None
    } else if let Some(vals) = p.get("values").and_then(Json::as_array) {
        Some(FilterCriterion::Values {
            values: vals.iter().map(crate::cmd::edit::json_to_input).collect(),
            blanks: bool_param(p, "blanks").unwrap_or(false),
        })
    } else if let Some(c) = p.get("custom") {
        let a =
            (c.get("op").and_then(Json::as_str).unwrap_or("=").to_string(), c.get("value").map(crate::cmd::edit::json_to_input).unwrap_or_default());
        let b =
            c.get("op2").and_then(Json::as_str).map(|o| (o.to_string(), c.get("value2").map(crate::cmd::edit::json_to_input).unwrap_or_default()));
        Some(FilterCriterion::Custom { a, b, and: c.get("and").and_then(Json::as_bool).unwrap_or(true) })
    } else if let Some(t) = p.get("top") {
        Some(FilterCriterion::Top10 {
            bottom: t.get("bottom").and_then(Json::as_bool).unwrap_or(false),
            percent: t.get("percent").and_then(Json::as_bool).unwrap_or(false),
            count: t.get("count").and_then(Json::as_u64).unwrap_or(10) as u32,
        })
    } else if let Some(c) = p.get("fillColor") {
        Some(FilterCriterion::FillColor(color_param(c, "data.filterBy")?))
    } else if let Some(c) = p.get("textColor") {
        Some(FilterCriterion::FontColor(color_param(c, "data.filterBy")?))
    } else {
        bool_param(p, "aboveAverage").map(FilterCriterion::AboveAverage)
    };
    let set = |criteria: &mut Vec<(u32, FilterCriterion)>| {
        criteria.retain(|(o, _)| *o != off);
        if let Some(c) = crit.clone() {
            criteria.push((off, c));
        }
    };
    edit(s, |cx| {
        match view {
            Some((id, _)) => {
                let sh = cx.sheet_mut(sheet)?;
                if let Some(v) = sh.filter_views.iter_mut().find(|v| v.id == id) {
                    set(&mut v.criteria);
                }
                // Shown here: show it as it now filters.
                if sh.view_rows.view == Some(id) {
                    show_filter_view(&mut cx.wb, sheet, Some(id));
                }
            }
            None => {
                let sh = cx.sheet_mut(sheet)?;
                if let Some(a) = sh.autofilter.as_mut() {
                    set(&mut a.criteria);
                }
                apply_filter(&mut cx.wb, sheet);
            }
        }
        cx.structural = true;
        Ok(Json::Null)
    })?;
    let hidden = s.doc()?.wb.sheet(sheet).map(|sh| sh.display_rows().values().filter(|r| r.hidden).count()).unwrap_or(0);
    Ok(json!({"hiddenRows": hidden}))
}

/// Does a value pass a custom comparison (`>`, `=*abc*`, `<>x`…)?
fn custom_ok(v: &Value, text: &str, op: &str, crit: &str) -> bool {
    let num = gridcraft_core::parse::parse_number_text(crit);
    let cmp = match (v.as_f64(), num) {
        (Some(a), Some(b)) => a.partial_cmp(&b),
        // A numeric ordering comparison only matches numbers; blanks and text never do.
        (None, Some(_)) if matches!(op, "<" | "<=" | ">" | ">=") => None,
        _ => Some(gridcraft_core::compare_text(text, crit)),
    };
    let wild = || crate::cmd::edit::wildcard(&text.to_lowercase(), &crit.to_lowercase());
    match op {
        "=" => {
            if crit.contains(['*', '?']) {
                wild()
            } else {
                cmp == Some(Ordering::Equal)
            }
        }
        "<>" => {
            if crit.contains(['*', '?']) {
                !wild()
            } else {
                cmp != Some(Ordering::Equal)
            }
        }
        ">" => cmp == Some(Ordering::Greater),
        "<" => cmp == Some(Ordering::Less),
        ">=" => matches!(cmp, Some(Ordering::Greater | Ordering::Equal)),
        "<=" => matches!(cmp, Some(Ordering::Less | Ordering::Equal)),
        "beginsWith" => text.to_lowercase().starts_with(&crit.to_lowercase()),
        "endsWith" => text.to_lowercase().ends_with(&crit.to_lowercase()),
        "contains" => text.to_lowercase().contains(&crit.to_lowercase()),
        "notContains" => !text.to_lowercase().contains(&crit.to_lowercase()),
        _ => true,
    }
}

/// The filter's "(Blanks)" group: truly empty cells and zero-length text such as a `=""` result.
fn filter_blank(v: &Value) -> bool {
    v.is_empty() || matches!(v, Value::Text(t) if t.is_empty())
}

/// Excel grows an AutoFilter range when a value is entered in the row right below it, in one of its columns (#2342).
/// A table's own filter is left to the table, and a row typed after a blank row does not count.
/// A formula counts before it is calculated: this runs inside the edit, ahead of the recalc.
pub(crate) fn grow_autofilter(sh: &mut Sheet, at: CellRef) {
    if !sh.cells.get(at).is_some_and(|c| !filter_blank(&c.value) || c.formula.is_some()) {
        return;
    }
    let Some(af) = sh.autofilter.as_ref() else { return };
    let r = af.range;
    if r.end.row.checked_add(1) != Some(at.row) || at.col < r.start.col || at.col > r.end.col || sh.tables.iter().any(|t| t.range == r) {
        return;
    }
    let grown = RangeRef::new(r.start, CellRef::new(at.row, r.end.col));
    if let Some(af) = sh.autofilter.as_mut() {
        af.range = grown;
    }
}

/// Hides rows of the autofilter range that fail any criterion.
pub(crate) fn apply_filter(wb: &mut Workbook, sheet: usize) {
    let Some(sh) = wb.sheet(sheet) else { return };
    let Some(af) = sh.autofilter.clone() else { return };
    let hide = hidden_by(wb, sh, &af);
    let Some(shm) = wb.sheet_mut(sheet) else { return };
    for (row, h) in hide {
        if h {
            shm.rows.entry(row).or_default().hidden = true;
        } else if let Some(e) = shm.rows.get_mut(&row) {
            e.hidden = false;
        }
    }
}

/// Each data row of `af.range` (below its header) and whether it fails any of `af.criteria`.
fn hidden_by(wb: &Workbook, sh: &Sheet, af: &AutoFilter) -> Vec<(u32, bool)> {
    let r = af.range;
    let mut hide = Vec::new();
    // Precompute per-criterion top10 threshold and average once, not per row.
    let mut col_stats: std::collections::HashMap<usize, (Option<f64>, f64)> = std::collections::HashMap::new();
    for (i, (off, c)) in af.criteria.iter().enumerate() {
        if matches!(c, FilterCriterion::Top10 { .. } | FilterCriterion::AboveAverage(_)) {
            let col = r.start.col + off;
            let mut nums: Vec<f64> = (r.start.row + 1..=r.end.row).filter_map(|row| sh.value(CellRef::new(row, col)).as_f64()).collect();
            let avg = if nums.is_empty() { 0.0 } else { nums.iter().sum::<f64>() / nums.len() as f64 };
            let thr = match c {
                FilterCriterion::Top10 { bottom, percent, count } => {
                    nums.sort_by(|a, b| if *bottom { a.partial_cmp(b) } else { b.partial_cmp(a) }.unwrap_or(Ordering::Equal));
                    let k = if *percent { ((nums.len() as f64) * (*count as f64) / 100.0).ceil() as usize } else { *count as usize };
                    let k = k.clamp(1, nums.len().max(1));
                    nums.get(k - 1).copied()
                }
                _ => None,
            };
            col_stats.insert(i, (thr, avg));
        }
    }
    for row in r.start.row + 1..=r.end.row {
        let mut visible = true;
        for (i, (off, crit)) in af.criteria.iter().enumerate() {
            let c = CellRef::new(row, r.start.col + off);
            let v = sh.value(c);
            let text = crate::display::cell_text(wb, sh, c);
            let ok = match crit {
                FilterCriterion::Values { values, blanks } => {
                    if filter_blank(&v) {
                        *blanks
                    } else {
                        values.iter().any(|x| x.eq_ignore_ascii_case(&text) || x.eq_ignore_ascii_case(&v.display()))
                    }
                }
                FilterCriterion::Custom { a, b, and } => {
                    let x = custom_ok(&v, &text, &a.0, &a.1);
                    match b {
                        Some(b) => {
                            let y = custom_ok(&v, &text, &b.0, &b.1);
                            if *and { x && y } else { x || y }
                        }
                        None => x,
                    }
                }
                FilterCriterion::Top10 { bottom, .. } => match (v.as_f64(), col_stats.get(&i)) {
                    (Some(n), Some((thr, _))) => {
                        let thr = thr.unwrap_or(n);
                        if *bottom { n <= thr } else { n >= thr }
                    }
                    _ => false,
                },
                FilterCriterion::AboveAverage(above) => match (v.as_f64(), col_stats.get(&i)) {
                    (Some(n), Some((_, avg))) => {
                        if *above {
                            n > *avg
                        } else {
                            n < *avg
                        }
                    }
                    _ => false,
                },
                FilterCriterion::FillColor(col) => cell_fill(wb, sh, c) == *col,
                FilterCriterion::FontColor(col) => wb.styles.get(sh.style_id(c)).font.color == *col,
                // Icons are not evaluated here: the row keeps its current visibility.
                FilterCriterion::Icon { .. } => !sh.rows.get(&row).is_some_and(|i| i.hidden),
            };
            if !ok {
                visible = false;
                break;
            }
        }
        hide.push((row, !visible));
    }
    hide
}

/// A cell's fill colour as filters and sorts see it: no fill (`Color::Auto`) unless it has a solid fill.
fn cell_fill(wb: &Workbook, sh: &Sheet, c: CellRef) -> Color {
    wb.styles.get(sh.style_id(c)).fill.fg
}

// ---------------------------------------------------------------- filter views

/// The rows of `sh` as filter view `view` shows them: in the view's data rows the view's own
/// filter decides; rows the sheet's filter hid show again, rows hidden by hand stay hidden.
pub fn view_rows(wb: &Workbook, sh: &Sheet, view: &FilterView) -> BTreeMap<u32, LineInfo> {
    let mut rows = sh.rows.clone();
    let spans = sh.filter_row_spans();
    let af = AutoFilter { range: view.range, criteria: view.criteria.clone() };
    for (row, fails) in hidden_by(wb, sh, &af) {
        let shared = sh.rows.get(&row).is_some_and(|i| i.hidden);
        let by_filter = shared && spans.iter().any(|&(a, b)| row >= a && row <= b);
        let hide = fails || (shared && !by_filter);
        if hide != shared {
            rows.entry(row).or_default().hidden = hide;
        }
    }
    rows
}

/// Shows filter view `id` on sheet `sheet` here (`None`: the sheet's own rows again). Nothing
/// saved or sent changes: only [`Sheet::view_rows`]. False when the sheet or the view isn't there.
/// A tab calls it again after anything changes the sheet, to show the view as it now filters.
pub fn show_filter_view(wb: &mut Workbook, sheet: usize, id: Option<u32>) -> bool {
    let Some(sh) = wb.sheet(sheet) else { return false };
    let shown = match id {
        None => ViewRows::default(),
        Some(id) => {
            let Some(v) = sh.filter_views.iter().find(|v| v.id == id) else { return false };
            ViewRows { view: Some(id), rows: Arc::new(view_rows(wb, sh, v)) }
        }
    };
    let Some(shm) = wb.sheet_mut(sheet) else { return false };
    shm.view_rows = shown;
    true
}

/// Data › Filter views › Create new filter view (`{name?, range?}`), Save as filter view
/// (`{fromFilter: true}`: the sheet's filter, its range and criteria) or Duplicate (`{copyOf: id}`).
/// Answers `{id, name, range}`.
pub(super) fn create_filter_view(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "data.createFilterView";
    let sheet = target_sheet(s, p)?;
    let sh = s.doc()?.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let copy_of = view_param(s, sheet, &json!({"view": p.get("copyOf")}), CMD)?;
    let (range, criteria, base) = if let Some((id, _)) = copy_of {
        let v = sh.filter_views.iter().find(|v| v.id == id).ok_or_else(|| bad(CMD, "no such filter view"))?;
        (v.range, v.criteria.clone(), format!("{} copy", v.name))
    } else if bool_param(p, "fromFilter").unwrap_or(false) {
        let af = sh.autofilter.as_ref().ok_or_else(|| bad(CMD, "this sheet has no filter to save"))?;
        (af.range, af.criteria.clone(), String::new())
    } else if p.get("range").is_none()
        && let Some(af) = &sh.autofilter
    {
        (af.range, vec![], String::new())
    } else {
        (new_filter_range(s, p, CMD)?.1, vec![], String::new())
    };
    let sh = s.doc()?.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let id = sh.filter_views.iter().map(|v| v.id).max().unwrap_or(0) + 1;
    let taken = |n: &str| sh.filter_views.iter().any(|v| v.name.eq_ignore_ascii_case(n));
    let name = match str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None if !base.is_empty() && !taken(base.as_str()) => base,
        None => (1..).map(|n| format!("Filter {n}")).find(|n| !taken(n.as_str())).unwrap_or_default(),
    };
    let view = FilterView { id, name: name.clone(), range, criteria };
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.filter_views.push(view.clone());
        Ok(json!({"id": id, "name": name, "range": range.a1()}))
    })
}

/// Renames a filter view or changes its range: `{view, name?, range?}`.
pub(super) fn update_filter_view(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "data.updateFilterView";
    let sheet = target_sheet(s, p)?;
    let (id, _) = view_param(s, sheet, p, CMD)?.ok_or_else(|| bad(CMD, "missing `view`"))?;
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
    let range = match str_param(p, "range") {
        Some(t) => Some(RangeRef::parse(t.trim_start_matches('=')).ok_or_else(|| bad(CMD, format!("`{t}` is not a range")))?),
        None => None,
    };
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        if let Some(v) = sh.filter_views.iter_mut().find(|v| v.id == id) {
            if let Some(n) = &name {
                v.name = n.clone();
            }
            if let Some(r) = range {
                // Criteria past the new range's last column no longer have a column.
                v.criteria.retain(|(o, _)| *o < r.width());
                v.range = r;
            }
        }
        if sh.view_rows.view == Some(id) {
            show_filter_view(&mut cx.wb, sheet, Some(id));
        }
        cx.structural = true;
        Ok(Json::Null)
    })
}

/// Deletes a filter view: `{view}`. A tab showing it goes back to the sheet's own rows.
pub(super) fn delete_filter_view(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "data.deleteFilterView";
    let sheet = target_sheet(s, p)?;
    let (id, _) = view_param(s, sheet, p, CMD)?.ok_or_else(|| bad(CMD, "missing `view`"))?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        sh.filter_views.retain(|v| v.id != id);
        if sh.view_rows.view == Some(id) {
            sh.view_rows = ViewRows::default();
        }
        cx.structural = true;
        Ok(Json::Null)
    })
}

/// The fill and text colours of a filter column's data rows (`{column, view?}`), for the filter
/// menu's Filter by colour and Sort by colour: `{fill: [{color, hex}], text: [{color, hex}]}`,
/// in the order they first appear. "No fill" is left out; automatic text shows as black.
pub(super) fn filter_colors_cmd(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "data.filterColors";
    let sheet = s.doc()?.wb.active_sheet();
    let view = view_param(s, sheet, p, CMD)?;
    let wb = &s.doc()?.wb;
    let sh = wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let range = match (view, &sh.autofilter) {
        (Some((_, r)), _) => r,
        (None, Some(a)) => a.range,
        (None, None) => return Err(bad(CMD, "this sheet has no filter")),
    };
    let col = col_param(p.get("column"), range.start.col).filter(|c| *c >= range.start.col && *c <= range.end.col);
    let col = col.ok_or_else(|| bad(CMD, "column outside the filter range"))?;
    let (fill, text) = filter_colors(wb, sh, range, col);
    let list = |colors: Vec<Color>| -> Vec<Json> {
        colors.into_iter().map(|c| json!({"color": c, "hex": c.hex(&wb.theme).unwrap_or_else(|| "#000000".into())})).collect()
    };
    Ok(json!({"fill": list(fill), "text": list(text)}))
}

/// The distinct fill colours (without "no fill") and text colours of column `col` in the data
/// rows of `range`, in the order they first appear; at most 64 of each.
pub fn filter_colors(wb: &Workbook, sh: &Sheet, range: RangeRef, col: u32) -> (Vec<Color>, Vec<Color>) {
    const MAX: usize = 64;
    let mut fill: Vec<Color> = Vec::new();
    let mut text: Vec<Color> = Vec::new();
    for row in range.start.row + 1..=range.end.row.min(range.start.row + 1_000_000) {
        let c = CellRef::new(row, col);
        let f = cell_fill(wb, sh, c);
        if f != Color::Auto && fill.len() < MAX && !fill.contains(&f) {
            fill.push(f);
        }
        let t = wb.styles.get(sh.style_id(c)).font.color;
        if text.len() < MAX && !text.contains(&t) {
            text.push(t);
        }
        if fill.len() >= MAX && text.len() >= MAX {
            break;
        }
    }
    (fill, text)
}

pub(super) fn clear_filter(s: &mut Session, _: &Json) -> Result<Json> {
    let sheet = s.doc()?.wb.active_sheet();
    crate::cmd::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        if let Some(a) = sh.autofilter.as_mut() {
            a.criteria.clear();
        }
        apply_filter(&mut cx.wb, sheet);
        cx.structural = true;
        Ok(Json::Null)
    })
}

pub(super) fn reapply(s: &mut Session, _: &Json) -> Result<Json> {
    let sheet = s.doc()?.wb.active_sheet();
    crate::cmd::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    edit(s, |cx| {
        apply_filter(&mut cx.wb, sheet);
        cx.structural = true;
        Ok(Json::Null)
    })
}

/// Distinct display values of a filter column (for the dropdown list).
pub fn filter_values(wb: &Workbook, sheet: usize, col: u32) -> Vec<(String, bool)> {
    let Some(sh) = wb.sheet(sheet) else { return vec![] };
    let Some(af) = sh.shown_filter() else { return vec![] };
    let off = col.saturating_sub(af.range.start.col);
    let selected: Option<&FilterCriterion> = af.criteria.iter().find(|(o, _)| *o == off).map(|(_, c)| c);
    let mut seen = std::collections::BTreeMap::new();
    for row in af.range.start.row + 1..=af.range.end.row.min(af.range.start.row + 1_000_000) {
        let c = CellRef::new(row, col);
        let v = sh.value(c);
        let blank = filter_blank(&v);
        let t = if blank { "(Blanks)".to_string() } else { crate::display::cell_text(wb, sh, c) };
        let on = match selected {
            Some(FilterCriterion::Values { values, blanks }) => {
                if blank {
                    *blanks
                } else {
                    values.iter().any(|x| x.eq_ignore_ascii_case(&t))
                }
            }
            _ => true,
        };
        let key = if blank { sort_key(&Value::Empty) } else { sort_key(&v) };
        seen.entry((key, t)).or_insert(on);
    }
    let mut values: Vec<_> = seen.into_iter().collect();
    values.sort_by(|((a, ta), _), ((b, tb), _)| {
        a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then_with(|| gridcraft_core::compare_text(&a.2, &b.2)).then_with(|| ta.cmp(tb))
    });
    values.into_iter().map(|((_, t), on)| (t, on)).collect()
}
