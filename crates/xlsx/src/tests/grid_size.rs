//! StarSheets 0110: a sheet's grid size travels in a worksheet extension, known by its uri; a
//! hostile or stale size never hides what the file holds.

use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef};
use gridcraft_model::{Comment, Sheet, Workbook};

use super::minimal;
use crate::{read_xlsx, write_xlsx};

fn ext(attrs: &str) -> String {
    format!(
        "<extLst><ext uri=\"{}\" xmlns:gc=\"urn:gridcraft:gridsize:1\"><gc:gridSize {attrs}/></ext></extLst>",
        crate::sheet_write::GRID_SIZE_EXT_URI
    )
}

fn size(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    let (wb, _) = read_xlsx(bytes).unwrap();
    let sh = wb.sheet(0).unwrap();
    (sh.grid_rows, sh.grid_cols)
}

#[test]
fn the_size_round_trips() {
    let mut wb = Workbook::new();
    let mut sh = Sheet::blank("S");
    sh.grid_rows = Some(50);
    sh.grid_cols = Some(5);
    wb.sheets = vec![std::sync::Arc::new(sh)];
    assert_eq!(size(&write_xlsx(&wb).unwrap()), (Some(50), Some(5)));
}

#[test]
fn a_hostile_size_is_brought_into_range() {
    let data = "<sheetData><row r=\"1\"><c r=\"A1\"><v>1</v></c></row></sheetData>";
    assert_eq!(size(&minimal(&format!("{data}{}", ext("rows=\"0\" cols=\"0\"")), &[], "", "")), (Some(1), Some(1)));
    assert_eq!(size(&minimal(&format!("{data}{}", ext("rows=\"4000000000\" cols=\"99999999\"")), &[], "", "")), (Some(MAX_ROWS), Some(MAX_COLS)));
    assert_eq!(size(&minimal(&format!("{data}{}", ext("rows=\"-5\" cols=\"x\"")), &[], "", "")), (None, None), "unreadable: the full grid");
}

#[test]
fn data_past_the_size_grows_it_on_read() {
    // Cells below and right of the stored size, a merge, and a conditional format further out.
    let body = format!(
        "<sheetData><row r=\"80\"><c r=\"C80\"><v>1</v></c></row></sheetData><mergeCells count=\"1\"><mergeCell ref=\"H2:J3\"/></mergeCells>\
         <conditionalFormatting sqref=\"A90:A95\"><cfRule type=\"cellIs\" dxfId=\"0\" priority=\"1\" operator=\"greaterThan\"><formula>5</formula></cfRule></conditionalFormatting>{}",
        ext("rows=\"50\" cols=\"5\"")
    );
    assert_eq!(size(&minimal(&body, &[], "", "")), (Some(95), Some(10)));
}

#[test]
fn notes_past_the_size_grow_it_through_a_round_trip() {
    let mut wb = Workbook::new();
    let mut sh = Sheet::blank("S");
    sh.grid_rows = Some(10);
    sh.grid_cols = Some(3);
    sh.comments.insert(
        CellRef::new(40, 6),
        Comment {
            author: "a".into(),
            text: "note".into(),
            replies: vec![],
            threaded: false,
            resolved: false,
            visible: false,
            dates: vec![],
            note_box: None,
        },
    );
    wb.sheets = vec![std::sync::Arc::new(sh)];
    assert_eq!(size(&write_xlsx(&wb).unwrap()), (Some(41), Some(7)));
}

#[test]
fn an_extension_with_another_uri_is_not_the_size() {
    let body =
        "<sheetData/><extLst><ext uri=\"{00000000-0000-0000-0000-000000000000}\" xmlns:gc=\"urn:other\"><gc:gridSize rows=\"5\"/></ext></extLst>";
    assert_eq!(size(&minimal(body, &[], "", "")), (None, None));
}

#[test]
fn whole_column_ranges_count_only_across() {
    let mut sh = Sheet::blank("S");
    sh.merges.push(RangeRef::cols(3, 4));
    assert_eq!(sh.content_end(), Some(CellRef::new(0, 4)));
}
