use super::cell_paint::ink_on;
use super::geo::Geo;
use super::{Align2, CellRef, Color32, FontId, PatternType, Rect, Sheet, SheetApp, Stroke, StrokeKind, Tokens, Workbook, json, pos2, theme, vec2};

/// Room the in-cell editor keeps right of its text. It is sized from the text of the last frame, so
/// the next typed character must still fit: a fixed 14 px was less than one glyph at high zoom,
/// and the field scrolled and snapped back on every key (#2336). `size` is the font size in px.
pub(super) fn editor_slack(size: f32) -> f32 {
    14.0 + size
}

/// Width of the one-line in-cell editor. While the text fits the cell (with the usual 14 px margin) the
/// editor is exactly the cell wide, as in Excel; once it overflows, the extra `editor_slack` applies.
pub(super) fn editor_width(text_w: f32, cell_w: f32, size: f32) -> f32 {
    if text_w + 14.0 <= cell_w { cell_w } else { text_w + editor_slack(size) }
}

/// Draws the in-cell editor, growing and wrapping with its text, mirroring for right-to-left sheets, choosing contrasting caret ink and clipping cells hidden under frozen panes.
pub(super) fn in_cell_editor(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, sh: &Sheet, wb: &Workbook) {
    let Some(ed) = app.editor.as_ref() else { return };
    if app.view_key() != Some((ed.doc, ed.sheet)) {
        return;
    }
    let cell = ed.cell;
    let rect = match sh.merge_at(cell) {
        Some(m) => geo.range_rect(sh, m),
        None => geo.cell_rect(sh, cell),
    };
    let st = wb.styles.get(sh.style_id(cell)).clone();
    let size = st.font.size * 96.0 / 72.0 * geo.z;
    // A formula is code, not the cell's text: it shows in the formula bar's plain font (never the
    // cell's bold, italic or typeface), at the cell's size so it still fits the cell, as in Sheets.
    let font = if ed.text.trim_start().starts_with('=') {
        theme::ui_font(size)
    } else {
        FontId::new(size, theme::cell_family(&st.font.name, st.font.bold, st.font.italic))
    };
    let lines = ed.text.lines().count().max(1) as f32 + if ed.text.ends_with('\n') { 1.0 } else { 0.0 };
    // Grows with the text: the in-cell editor shows every line and never scrolls (the formula bar
    // is the one that stops at five lines).
    let shown = lines;
    // Wrap Text: the editor keeps the column's width and wraps, growing down as Excel's does.
    // Otherwise it widens to the text's one-line width.
    let wrap = st.align.wrap;
    // StarSheets (to-do 2 rows 16/17): the editor grows with the formula as far as the grid
    // allows, as in Sheets: wider first (a Wrap cell too, not only its column), then, at the
    // grid's right edge, wrapping and growing down.
    let room_right = (geo.cells.right() - rect.left()).max(rect.width());
    let room_down = (geo.cells.bottom() - rect.top()).max(rect.height());
    let text_w = ui.painter().layout_no_wrap(ed.text.clone(), font.clone(), Color32::BLACK).size().x;
    let want = editor_width(text_w, rect.width(), size);
    if (wrap && want > rect.width()) || want > room_right {
        let w = want.min(room_right);
        let wrapped = ui.painter().layout(ed.text.clone(), font.clone(), Color32::BLACK, (w - 4.0).max(1.0));
        // Room for the next line while it wraps, so the field never scrolls.
        let next = if want > room_right { size } else { 0.0 };
        let h = (wrapped.size().y.max(size * 1.3 * shown) + next + 4.0).max(rect.height()).min(room_down);
        return draw_in_cell_editor(app, ui, geo, sh, wb, cell, rect, &st, font, true, w, h);
    }
    let (w, h) = if wrap {
        // The field wraps at the width it is given (the editor's less its 2 px inset a side): its
        // frame is `Frame::NONE`, so no text margin is taken off.
        let wrapped = ui.painter().layout(ed.text.clone(), font.clone(), Color32::BLACK, (rect.width() - 4.0).max(1.0));
        (rect.width(), (wrapped.size().y.max(size * 1.3 * shown) + 4.0).max(rect.height()))
    } else {
        let text_w = ui.painter().layout_no_wrap(ed.text.clone(), font.clone(), Color32::BLACK).size().x;
        // Excel keeps the cell's height for one line and only grows for extra lines.
        let h = if lines > 1.0 { (size * 1.3 * lines + 4.0).max(rect.height()) } else { rect.height() };
        (editor_width(text_w, rect.width(), size).min(geo.cells.right() - rect.left()), h)
    };
    draw_in_cell_editor(app, ui, geo, sh, wb, cell, rect, &st, font, wrap, w, h);
}

/// Paints the in-cell editor `w` x `h` from the cell's top left and runs the text field in it.
#[allow(clippy::too_many_arguments)]
fn draw_in_cell_editor(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, sh: &Sheet, wb: &Workbook, cell: CellRef, rect: Rect, st: &gridcraft_engine::model::Style, font: FontId, wrap: bool, w: f32, h: f32) {
    // Grows to the left on a right-to-left sheet.
    let erect = geo.mirror_rect(Rect::from_min_size(rect.min, vec2(w, h)));
    let fill = match st.fill.pattern {
        PatternType::Solid => st.fill.fg.resolve(&wb.theme).map(theme::color32).unwrap_or(Color32::WHITE),
        _ => Color32::WHITE,
    };
    let split = pos2(geo.cells.left() + geo.frozen_w, geo.cells.top() + geo.frozen_h);
    let quadrant = Rect::from_min_max(
        pos2(if cell.col >= geo.fc { split.x } else { geo.cells.left() }, if cell.row >= geo.fr { split.y } else { geo.cells.top() }),
        pos2(if cell.col >= geo.fc { geo.cells.right() } else { split.x }, if cell.row >= geo.fr { geo.cells.bottom() } else { split.y }),
    );
    // A cell scrolled entirely under a frozen row/column keeps its edit (Escape still works),
    // but neither the cell nor its helper may paint. Test the cell's own visible quadrant and
    // give the shared editor an empty clip when the cell is hidden: a one-line cell editor's
    // field can be larger than the cell (egui 0.37 gives text fields a minimum height), so its
    // own max_rect is not a reliable "is the cell on screen" test at the frozen boundary. The
    // pre-wrap cell rect must also be positive: a hidden row/column has zero height/width, but
    // Wrap Text can expand the editor below/right of it, which must not paint over the next line.
    let cell_visible = rect.width() > 0.0 && rect.height() > 0.0;
    // `erect` and `ui.clip_rect()` are in screen coordinates; `quadrant` is logical until mirrored.
    let screen_quadrant = geo.mirror_rect(quadrant);
    let on_screen = cell_visible && erect.intersect(screen_quadrant).is_positive() && !geo.hidden_under_frozen(sh, cell);
    let clip = if on_screen { ui.clip_rect().intersect(screen_quadrant) } else { Rect::NOTHING };
    let painter = ui.painter().with_clip_rect(clip);
    painter.rect_filled(erect, 0.0, fill);
    painter.rect_stroke(erect.expand(1.0), 0.0, Stroke::new(2.0, Tokens::get(ui.ctx()).sel_border), StrokeKind::Middle);
    let id = egui::Id::unique("gridcraft.cell_editor");
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(erect.shrink2(vec2(2.0, 1.0))));
    child.set_clip_rect(clip);
    // The theme's caret is meant for the UI chrome and vanishes on some fills (and, in the dark
    // theme, on the white paper): pick black or white against the fill, like Excel.
    child.visuals_mut().text_cursor.stroke.color = ink_on(fill);
    // The cell's own font colour for its text while editing (as in Sheets); a formula keeps the
    // editor's plain colours.
    let ink = st.font.color.resolve(&wb.theme).map(theme::color32);
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(crate::formula_bar::CELL_INK), ink));
    crate::formula_bar::editor_widget(app, &mut child, id, font, wrap, erect.width() - 4.0, erect.height() - 2.0);
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("gridcraft.cell_editor.rect"), erect));
}

/// Test the popup itself, not whether the pointer is over any part of the app.
/// A trigger can sit outside its popup, so its opening click must be ignored.
pub(super) fn popup_dismissed(response: &egui::Response, was_open: bool) -> bool {
    response.ctx.input(|i| !i.focused || i.key_pressed(egui::Key::Escape)) || (was_open && response.clicked_elsewhere())
}

/// The dropdown arrow beside a cell with an in-cell list validation.
pub fn validation_arrow(sh: &Sheet, geo: &Geo, c: CellRef) -> Option<Rect> {
    let dv = sh.validations.iter().find(|d| d.ranges.iter().any(|r| r.contains(c)))?;
    if dv.kind != gridcraft_engine::model::ValidationKind::List || !dv.in_cell_dropdown || geo.hidden_under_frozen(sh, c) {
        return None;
    }
    let r = geo.cell_rect(sh, c);
    let s = r.height().clamp(14.0, 22.0);
    Some(Rect::from_min_size(pos2(r.right() + 1.0, r.bottom() - s), vec2(s, s)))
}

/// Sheets' ▾ inside the right edge of any drop-down cell (drawn by the quadrant painter): its click area.
pub fn sheets_dropdown_hit(sh: &Sheet, geo: &Geo, c: CellRef) -> Option<Rect> {
    let dv = sh.validations.iter().find(|d| d.ranges.iter().any(|r| r.contains(c)))?;
    let plain = dv.display == gridcraft_engine::model::DropdownDisplay::Plain;
    if dv.kind != gridcraft_engine::model::ValidationKind::List || !dv.in_cell_dropdown || plain || geo.hidden_under_frozen(sh, c) {
        return None;
    }
    let r = geo.cell_rect(sh, c);
    Some(Rect::from_min_max(pos2(r.right() - 20.0, r.top()), r.right_bottom()))
}

/// Rows the list-validation dropdown shows before it scrolls (Excel shows 8).
const LIST_PICKER_MAX_ROWS: usize = 8;

const LIST_PICKER_ROW_H: f32 = 18.0;

/// Popup frame around the rows: 2 px inner margin and the window stroke on each side, rounded up.
const LIST_PICKER_CHROME: f32 = 8.0;

/// Popup height for `n` items: every item up to `LIST_PICKER_MAX_ROWS`, then a scrollbar.
pub(super) fn list_picker_height(n: usize, row_h: f32, gap: f32) -> f32 {
    let rows = n.clamp(1, LIST_PICKER_MAX_ROWS) as f32;
    rows * row_h + (rows - 1.0) * gap
}

/// Draws the validation list popup, choosing above/below and left/right placement for RTL, and applies the clicked item with `cell.set`.
pub(super) fn list_picker(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, was_open: bool) {
    let Some(c) = app.grid.list_picker else { return };
    let Some(d) = app.session.active() else { return };
    let wb = d.wb.clone();
    let si = wb.active_sheet();
    let Some(sh) = wb.sheet(si) else { return };
    let Some(dv) = sh.validations.iter().find(|x| x.ranges.iter().any(|r| r.contains(c))).cloned() else {
        app.grid.list_picker = None;
        return;
    };
    let items = gridcraft_engine::cmd::data::list_items(&wb, si, c, &dv);
    // Multiple selections (Sheets): each row is a tick box; the cell holds the ticked items.
    let current = sh.input_text(c);
    let picked: Vec<&str> = if dv.multiple { gridcraft_engine::model::split_selections(&current) } else { Vec::new() };
    let is_picked = |it: &str| picked.iter().any(|p| p.eq_ignore_ascii_case(it));
    let r = geo.cell_rect(sh, c);
    let screen = ui.ctx().content_rect();
    // A host's menu look (StarSheets): every choice shown, as tall as the window allows below
    // or above the cell (whichever has more room), then a scroll bar that is always visible.
    let look = app.ui.menu_look;
    let (height, above) = if look.is_some() {
        let full = items.len().max(1) as f32 * 32.0;
        let room_below = (screen.bottom() - r.bottom() - LIST_PICKER_CHROME - 16.0).max(32.0);
        let room_above = (r.top() - screen.top() - LIST_PICKER_CHROME - 16.0).max(32.0);
        let above = full > room_below && room_above > room_below;
        (full.min(if above { room_above } else { room_below }), above)
    } else {
        // Fit the popup to its rows (up to LIST_PICKER_MAX_ROWS); without a minimum the scroll area collapses to its 64 px default.
        let height = list_picker_height(items.len(), LIST_PICKER_ROW_H, ui.spacing().item_spacing.y);
        // Open below the cell, or above it when the rows would run past the bottom of the window (as Excel does).
        (height, r.bottom() + height + LIST_PICKER_CHROME > screen.bottom() && r.top() - height - LIST_PICKER_CHROME >= screen.top())
    };
    let (at, pivot) = match (above, geo.rtl) {
        (false, false) => (geo.mirror_pos(r.left_bottom()), Align2::LEFT_TOP),
        (false, true) => (geo.mirror_pos(r.left_bottom()), Align2::RIGHT_TOP),
        (true, false) => (geo.mirror_pos(r.left_top()), Align2::LEFT_BOTTOM),
        (true, true) => (geo.mirror_pos(r.left_top()), Align2::RIGHT_BOTTOM),
    };
    let mut close = false;
    // Keyboard (Sheets): ↓ ↑ move through the choices (and the host's "Edit rule" row), Enter
    // picks, Esc closes (see popup_dismissed).
    let rows = items.len() + usize::from(look.is_some());
    let (down, up, enter) = ui.input_mut(|i| {
        (i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown), i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
    });
    if rows > 0 && down {
        app.grid.list_key = Some(app.grid.list_key.map_or(0, |k| (k + 1).min(rows - 1)));
    }
    if rows > 0 && up {
        app.grid.list_key = Some(app.grid.list_key.map_or(0, |k| k.saturating_sub(1)));
    }
    let key = app.grid.list_key;
    let mut chosen = if enter { key } else { None };
    close |= enter && key.is_none();
    let response = egui::Area::new(egui::Id::unique("dv_list")).fixed_pos(at).pivot(pivot).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        let frame = if look.is_some() { egui::Frame::menu(ui.style()) } else { egui::Frame::popup(ui.style()).inner_margin(2.0) };
        frame.show(ui, |ui| {
            let width = match look {
                Some(l) => (l.list_style)(ui, &items),
                None => r.width().max(100.0),
            };
            ui.set_min_width(width);
            let visibility = if look.is_some() { egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded } else { egui::scroll_area::ScrollBarVisibility::default() };
            egui::ScrollArea::vertical().max_height(height).min_scrolled_height(height).scroll_bar_visibility(visibility).show(ui, |ui| {
                for (k, it) in items.iter().enumerate() {
                    let on = key == Some(k);
                    let row = match look {
                        _ if dv.multiple => {
                            let mut ticked = is_picked(it.as_str());
                            let h = if look.is_some() { 32.0 } else { LIST_PICKER_ROW_H };
                            ui.add_sized([width, h], egui::Checkbox::new(&mut ticked, it.as_str()))
                        }
                        Some(l) => ui.add_sized([width, 32.0], (l.list_row)(it.as_str()).selected(on)),
                        None => ui.add(egui::Button::new(it.as_str()).frame_when_inactive(false).selected(on).min_size(vec2(width, LIST_PICKER_ROW_H))),
                    };
                    if on && (up || down) {
                        row.scroll_to_me(None);
                    }
                    if row.clicked() {
                        chosen = Some(k);
                    }
                }
            });
            // Sheets: under the choices, a way to the rule itself.
            if let Some(l) = look {
                (l.sep)(ui);
                if ui.add_sized([width, 32.0], (l.list_row)("Edit rule").wrap_mode(egui::TextWrapMode::Extend).selected(key == Some(items.len()))).clicked() {
                    chosen = Some(items.len());
                }
            }
        });
    });
    match chosen {
        // Multiple selections: a pick ticks or unticks the item and the list stays open; the
        // ticked items are kept in the list's order.
        Some(k) if k < items.len() && dv.multiple => {
            let flip = items[k].as_str();
            let ticked = |it: &&str| it.eq_ignore_ascii_case(flip) != is_picked(*it);
            let now: Vec<&str> = items.iter().map(String::as_str).filter(ticked).collect();
            app.run_or_alert("cell.set", json!({"cell": c.a1(), "input": gridcraft_engine::model::join_selections(&now)}));
        }
        Some(k) if k < items.len() => {
            app.run_or_alert("cell.set", json!({"cell": c.a1(), "input": items[k]}));
            close = true;
        }
        Some(_) => {
            app.grid.edit_validation = Some(c);
            close = true;
        }
        None => {}
    }
    if close || popup_dismissed(&response.response, was_open) {
        app.grid.list_picker = None;
        app.grid.list_key = None;
    }
}
