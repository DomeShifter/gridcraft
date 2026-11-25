//! Filter views (Google Sheets' Data › Filter views), which Excel has no place for, survive a save
//! and a reopen in a worksheet extension Excel skips; colour filters stay Excel's `colorFilter`.
//! What one tab shows of a view never reaches the file.

use std::collections::BTreeMap;
use std::sync::Arc;

use gridcraft_core::RangeRef;
use gridcraft_model::{AutoFilter, Color, FilterCriterion, FilterView, LineInfo, Sheet, ViewRows, Workbook};

use crate::package::Package;
use crate::{read_xlsx, write_xlsx};

fn rr(a: &str) -> RangeRef {
    RangeRef::parse(a).unwrap()
}

fn sheet_xml(bytes: &[u8]) -> String {
    let mut pkg = Package::open(bytes).unwrap();
    String::from_utf8(pkg.read("xl/worksheets/sheet1.xml").unwrap().unwrap()).unwrap()
}

#[test]
fn filter_views_round_trip_in_an_extension() {
    let mut wb = Workbook::default();
    let mut s = Sheet::new("Data");
    s.autofilter = Some(AutoFilter { range: rr("A1:C9"), criteria: vec![(2, FilterCriterion::FontColor(Color::Rgb(0x0000FF)))] });
    s.filter_views = vec![
        FilterView {
            id: 1,
            name: "Reds & <blues>".into(),
            range: rr("A1:C9"),
            criteria: vec![
                (1, FilterCriterion::Values { values: vec!["Red".into(), "Blue".into()], blanks: true }),
                (0, FilterCriterion::FillColor(Color::Rgb(0x00FF00))),
            ],
        },
        FilterView { id: 4, name: "Everything".into(), range: rr("B2:D20"), criteria: vec![] },
    ];
    // Shown in this tab: hides row 3, which must not be saved hidden.
    let mut shown = BTreeMap::new();
    shown.insert(3, LineInfo { hidden: true, ..Default::default() });
    s.view_rows = ViewRows { view: Some(1), rows: Arc::new(shown) };
    wb.sheets = vec![Arc::new(s)];
    let bytes = write_xlsx(&wb).unwrap();

    let xml = sheet_xml(&bytes);
    // Excel's own filter keeps its colour filter (a text colour: cellColor="0").
    assert!(xml.contains("<autoFilter ref=\"A1:C9\"><filterColumn colId=\"2\"><colorFilter dxfId="), "{xml}");
    assert!(xml.contains("cellColor=\"0\"/>"), "{xml}");
    let ext = xml.find("{5D7B2E94-3C1A-4E8F-9B06-7A4C2D1E8F35}").expect("filter views extension");
    assert!(ext > xml.find("<extLst>").unwrap(), "the views sit in the sheet's extLst");
    assert!(xml.contains("name=\"Reds &amp; &lt;blues&gt;\""), "{xml}");
    assert!(!xml.contains("hidden=\"1\""), "a view shown in one tab hides nothing in the file: {xml}");
    // Only the sheet's own filter is an autoFilter Excel reads; the views' are inside the extension.
    assert_eq!(xml[..ext].matches("<autoFilter").count(), 1);

    let (back, _) = read_xlsx(&bytes).unwrap();
    let sh = &back.sheets[0];
    assert_eq!(sh.filter_views, wb.sheets[0].filter_views);
    assert_eq!(sh.autofilter, wb.sheets[0].autofilter);
    assert_eq!(sh.view_rows.view, None, "opening a file shows no view");

    // And again: the views don't pile up or change on a second save.
    let (again, _) = read_xlsx(&write_xlsx(&back).unwrap()).unwrap();
    assert_eq!(again.sheets[0].filter_views, wb.sheets[0].filter_views);
}

#[test]
fn a_sheet_without_views_gets_no_extension() {
    let mut wb = Workbook::default();
    wb.sheets = vec![Arc::new(Sheet::new("Data"))];
    let xml = sheet_xml(&write_xlsx(&wb).unwrap());
    assert!(!xml.contains("filterViews"), "{xml}");
}

#[test]
fn a_fill_colour_filter_round_trips_as_excels_color_filter() {
    let mut wb = Workbook::default();
    let mut s = Sheet::new("Data");
    s.autofilter = Some(AutoFilter { range: rr("A1:B5"), criteria: vec![(0, FilterCriterion::FillColor(Color::Rgb(0xFFC000)))] });
    wb.sheets = vec![Arc::new(s)];
    let bytes = write_xlsx(&wb).unwrap();
    let xml = sheet_xml(&bytes);
    assert!(xml.contains("<colorFilter dxfId=\"") && !xml.contains("cellColor=\"0\""), "{xml}");
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheets[0].autofilter, wb.sheets[0].autofilter);
}
