//! Google Sheets' SPARKLINE and IMAGE drawn in their cells (StarSheets 0102). The functions give
//! the cell no value; the grid evaluates the formula's arguments here and draws a small chart
//! (line, column, win/loss or bar) or the picture at a web address.
//!
//! Pictures come from the host: the grid asks for each address it meets ([`wanted`]), the host
//! fetches it and hands the bytes back ([`set`]), and the next frame draws it. Nothing is fetched
//! by the grid itself, so a host decides how (and whether) to reach the web.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Color32, Painter, Rect, Stroke, StrokeKind, pos2, vec2};
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::formula::Expr;
use gridcraft_engine::model::{Alignment, CellPicture, Sheet, SparklineKind, Workbook};

/// The most addresses kept (a sheet listing thousands of pictures keeps the newest).
const MAX_IMAGES: usize = 512;

enum Slot {
    Wanted,
    Asked,
    Ready(Arc<CellPicture>),
    Failed,
}

static IMAGES: Mutex<Option<HashMap<String, Slot>>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut HashMap<String, Slot>) -> R) -> R {
    let mut g = IMAGES.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

/// Addresses the grid wants pictures for and nobody has asked for yet; each is returned once.
pub fn wanted() -> Vec<String> {
    with(|m| {
        let out: Vec<String> = m.iter().filter(|(_, s)| matches!(s, Slot::Wanted)).map(|(u, _)| u.clone()).collect();
        for u in &out {
            m.insert(u.clone(), Slot::Asked);
        }
        out
    })
}

/// The host's answer for an address: the picture's bytes and type, or `None` when it couldn't
/// be had (the cell then shows a small broken-picture mark).
pub fn set(url: &str, picture: Option<(Vec<u8>, String)>) {
    with(|m| {
        if m.len() >= MAX_IMAGES && !m.contains_key(url) {
            m.clear();
        }
        let slot = match picture {
            Some((data, mime)) => Slot::Ready(Arc::new(CellPicture { data, mime, alt: url.to_string() })),
            None => Slot::Failed,
        };
        m.insert(url.to_string(), slot);
    });
}

/// Whether the picture at `url` has arrived (for tests and hosts).
pub fn ready(url: &str) -> bool {
    with(|m| matches!(m.get(url), Some(Slot::Ready(_))))
}

/// The function a cell's formula is, when it's SPARKLINE or IMAGE, with its arguments.
fn graphic_call(sh: &Sheet, c: CellRef) -> Option<(bool, Vec<Expr>)> {
    let f = sh.cell(c)?.formula.as_ref()?;
    let head = f.text.trim_start_matches('=').trim_start().get(..6)?.to_ascii_uppercase();
    if head != "SPARKL" && !head.starts_with("IMAGE") {
        return None;
    }
    match &*f.expr_arc()? {
        Expr::Call(n, args) if n.eq_ignore_ascii_case("SPARKLINE") => Some((true, args.clone())),
        Expr::Call(n, args) if n.eq_ignore_ascii_case("IMAGE") => Some((false, args.clone())),
        _ => None,
    }
}

/// Draws the SPARKLINE and IMAGE cells among `cells` (cell, its rectangle on screen).
pub(crate) fn paint(p: &Painter, wb: &Workbook, si: usize, sh: &Sheet, cells: impl Iterator<Item = (CellRef, Rect)>) {
    for (c, rect) in cells {
        let Some((spark, args)) = graphic_call(sh, c) else { continue };
        // The function's empty result (calc may keep it as empty text or as nothing); an error
        // shows as the error.
        let v = sh.value(c);
        if !(v.is_empty() || matches!(&v, Value::Text(t) if t.is_empty())) {
            continue;
        }
        let eval = |i: usize| args.get(i).map(|e| gridcraft_engine::calc::recalc::evaluate_expr(wb, si, c, e));
        if spark {
            let data = eval(0).unwrap_or(Value::Empty);
            let options = eval(1).map(|v| options(&v)).unwrap_or_default();
            paint_sparkline(p, rect.shrink(2.0), &numbers(&data), &options, wb);
        } else {
            let url = match eval(0) {
                Some(Value::Text(t)) => t.trim().to_string(),
                _ => continue,
            };
            let mode = eval(1).and_then(|v| v.as_f64()).unwrap_or(1.0) as i64;
            let size = (eval(2).and_then(|v| v.as_f64()), eval(3).and_then(|v| v.as_f64()));
            paint_image(p, rect, &url, mode, size);
        }
    }
}

fn numbers(v: &Value) -> Vec<Option<f64>> {
    match v {
        Value::Array(a) => a.data.iter().map(|x| if matches!(x, Value::Number(_)) { x.as_f64() } else { None }).collect(),
        Value::Number(n) => vec![Some(*n)],
        _ => vec![],
    }
}

/// Google's options: a two-column array of name and value ({"charttype","column";"color","red"}).
fn options(v: &Value) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    if let Value::Array(a) = v
        && a.cols >= 2
    {
        for r in 0..a.rows {
            if let Value::Text(k) = &a.data[r * a.cols] {
                out.insert(k.to_ascii_lowercase(), a.data[r * a.cols + 1].clone());
            }
        }
    }
    out
}

/// A colour by name or #RRGGBB, as Google's options take them.
fn colour(v: Option<&Value>, default: [u8; 3]) -> [u8; 3] {
    let Some(Value::Text(t)) = v else { return default };
    let t = t.trim().to_ascii_lowercase();
    if let Some(gridcraft_engine::model::Color::Rgb(rgb)) = gridcraft_engine::model::Color::from_hex(&t) {
        return [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
    }
    match t.as_str() {
        "red" => [0xEA, 0x43, 0x35],
        "green" => [0x34, 0xA8, 0x53],
        "blue" => [0x42, 0x85, 0xF4],
        "orange" => [0xFB, 0x8C, 0x00],
        "yellow" => [0xFB, 0xBC, 0x04],
        "purple" => [0x8E, 0x24, 0xAA],
        "black" => [0, 0, 0],
        "grey" | "gray" => [0x80, 0x80, 0x80],
        "white" => [0xFF, 0xFF, 0xFF],
        _ => default,
    }
}

fn paint_sparkline(p: &Painter, rect: Rect, vals: &[Option<f64>], o: &HashMap<String, Value>, _wb: &Workbook) {
    let kind = match &o.get("charttype") {
        Some(Value::Text(t)) => t.to_ascii_lowercase(),
        _ => "line".into(),
    };
    const BLUE: [u8; 3] = [0x42, 0x85, 0xF4];
    if kind == "bar" {
        // One or two values as a stacked horizontal bar against max (default: their sum).
        let a = vals.first().copied().flatten().unwrap_or(0.0).max(0.0);
        let b = vals.get(1).copied().flatten().unwrap_or(0.0).max(0.0);
        let max = o.get("max").and_then(Value::as_f64).unwrap_or(a + b).max(f64::EPSILON);
        let c1 = colour(o.get("color1"), BLUE);
        let c2 = colour(o.get("color2"), [0xEA, 0x43, 0x35]);
        let h = rect.height().min(18.0);
        let top = rect.center().y - h / 2.0;
        let wa = (a / max).clamp(0.0, 1.0) as f32 * rect.width();
        let wb = ((b / max).clamp(0.0, 1.0) as f32 * rect.width()).min(rect.width() - wa);
        p.rect_filled(Rect::from_min_size(pos2(rect.left(), top), vec2(wa, h)), 0.0, Color32::from_rgb(c1[0], c1[1], c1[2]));
        if wb > 0.0 {
            p.rect_filled(Rect::from_min_size(pos2(rect.left() + wa, top), vec2(wb, h)), 0.0, Color32::from_rgb(c2[0], c2[1], c2[2]));
        }
        return;
    }
    let kind = match kind.as_str() {
        "column" => SparklineKind::Column,
        "winloss" => SparklineKind::WinLoss,
        _ => SparklineKind::Line,
    };
    let c = colour(o.get("color").or_else(|| o.get("color1")), BLUE);
    let prims = gridcraft_chart::render_sparkline(kind, vals, [c[0], c[1], c[2], 255], false, rect.width(), rect.height());
    crate::chartview::paint_prims(p, rect.min, &prims, 1.0);
}

fn paint_image(p: &Painter, rect: Rect, url: &str, mode: i64, size: (Option<f64>, Option<f64>)) {
    let slot = with(|m| match m.get(url) {
        Some(Slot::Ready(pic)) => Some(Ok(pic.clone())),
        Some(Slot::Failed) => Some(Err(())),
        Some(_) => None,
        None => {
            m.insert(url.to_string(), Slot::Wanted);
            p.ctx().request_repaint();
            None
        }
    });
    match slot {
        Some(Ok(pic)) => {
            let area = match (mode, size) {
                // 4: custom size in pixels, at the top left.
                (4, (Some(h), Some(w))) => Rect::from_min_size(rect.min, vec2(w as f32, h as f32)).intersect(rect),
                _ => rect,
            };
            let align = Alignment::default();
            // 2 stretches to the cell; 1 (default), 3 and 4 keep the picture's shape.
            crate::cell_pictures::paint_as(p, area, &pic, &align, mode == 2);
        }
        Some(Err(())) => {
            let r = Rect::from_center_size(rect.center(), vec2(12.0, 10.0));
            p.rect_stroke(r, 1.0, Stroke::new(1.0, Color32::GRAY), StrokeKind::Inside);
            p.line_segment([r.left_bottom(), r.right_top()], Stroke::new(1.0, Color32::GRAY));
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_and_colours_read_googles_forms() {
        let a = gridcraft_engine::core::Array::new(
            2,
            2,
            vec![Value::text("charttype"), Value::text("column"), Value::text("Color"), Value::text("#00FF00")],
        )
        .unwrap();
        let o = options(&Value::Array(Arc::new(a)));
        assert_eq!(o.get("charttype"), Some(&Value::text("column")));
        assert_eq!(colour(o.get("color"), [0, 0, 0]), [0, 0xFF, 0]);
        assert_eq!(colour(Some(&Value::text("red")), [0, 0, 0]), [0xEA, 0x43, 0x35]);
        assert_eq!(numbers(&Value::Number(2.0)), vec![Some(2.0)]);
    }

    #[test]
    fn the_host_answers_for_each_address_once() {
        let url = "https://example.org/a.png";
        with(|m| {
            m.insert(url.into(), Slot::Wanted);
        });
        assert!(wanted().contains(&url.to_string()));
        assert!(!wanted().contains(&url.to_string()), "asked once");
        set(url, Some((vec![1, 2, 3], "image/png".into())));
        assert!(ready(url));
    }
}
