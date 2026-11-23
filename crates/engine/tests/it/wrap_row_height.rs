//! #1969: a wrapped cell's row must fit WORD-wrapped lines, not the character-fit
//! estimate (words can't split, so each drawn line ends early and narrow columns
//! need more lines than width/estimate suggests).

use gridcraft_engine::Session;
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn row_height(s: &mut Session, row: u32) -> f32 {
    s.doc().unwrap().wb.active().unwrap().row_height(row)
}

#[test]
fn wrapped_row_fits_word_wrapped_lines() {
    let mut s = session();
    // Column fits ~10 characters; six-character words mean only one word per line.
    s.execute("home.columnWidth", json!({"cols": "A:A", "chars": 10})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "aaaaaa bbbbbb cccccc dddddd"})).unwrap();
    let plain = row_height(&mut s, 0);
    s.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
    let wrapped = row_height(&mut s, 0);
    // Word wrap: 4 lines (one word each). The old estimate: ceil(28/10) = 3 lines.
    // Line height at 11pt is ~19.8, so 4 lines need ~79 vs ~59 for 3.
    assert!(wrapped >= plain + 3.0 * 19.0, "4 word-wrapped lines must grow the row by ~4 line heights: plain={plain}, wrapped={wrapped}");
}

#[test]
fn issue_repro_nasa_sentence() {
    let mut s = session();
    s.execute("home.columnWidth", json!({"cols": "A:A", "chars": 12})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "Since the 1960s, NASA has left hardware across the solar system\u{2013}on the Moon, on Mars, and in deep space."}))
        .unwrap();
    s.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
    let h = row_height(&mut s, 0);
    // Excel shows this at ~10 lines in a 12-char column; the character estimate undercounts.
    assert!(h >= 9.0 * 19.0, "NASA sentence in a 12-char column needs ~10 lines, got height {h}");
}

#[test]
fn long_wrapped_sentence_gets_no_extra_blank_line() {
    // A wide column of 10 pt prose that draws on 3 lines in Roboto/Arial. The generous autofit
    // widths (a space at 0.38 em) asked for 4, and bottom-aligned text showed a blank line above.
    let mut s = session();
    // 526 px: the old widths asked for 4 lines here, the text draws on 3.
    s.execute("home.columnWidth", json!({"cols": "A:A", "width": 526})).unwrap();
    s.execute("home.fontSize", json!({"range": "A1", "size": 10})).unwrap();
    s.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
    s.execute(
        "cell.set",
        json!({"cell": "A1", "input": "Pressing Ctrl+Enter inside a cell should go to a new line below (as it does in GSheets) - Related: this cell is an example of a bug where pressing Ctrl_Enter a few times seesm to have left a space above the text I have typed that I can't remove."}),
    )
    .unwrap();
    let h = row_height(&mut s, 0);
    let line = 10.0 * 96.0 / 72.0 * 1.22;
    assert!(h < 3.0 * line + 6.0, "3 drawn lines must give a 3-line row, got height {h}");
    assert!(h > 2.0 * line + 6.0, "the row must still fit 3 lines, got height {h}");
}
