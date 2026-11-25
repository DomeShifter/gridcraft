//! Drop-down settings Excel has no place for (multiple selections, display style, chip colours)
//! and value axis bounds survive a save and a reopen; the file stays one Excel reads.

use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::{
    Anchor, AxisBounds, Chart, ChartKind, Color, DropdownDisplay, ItemColor, LegendPos, Series, Sheet, Validation, ValidationKind, Workbook,
};

use crate::package::Package;
use crate::{read_xlsx, write_xlsx};

fn rr(a: &str) -> RangeRef {
    RangeRef::parse(a).unwrap()
}

fn chart(kind: ChartKind, value_axis: AxisBounds) -> Chart {
    Chart {
        id: 1,
        kind,
        anchor: Anchor { cell: CellRef::new(0, 3), width: 480.0, height: 300.0, ..Anchor::default() },
        title: None,
        series: vec![Series {
            name: None,
            categories: Some("Data!$A$2:$A$5".into()),
            values: "Data!$B$2:$B$5".into(),
            bubble_sizes: None,
            color: None,
            secondary: false,
            kind: None,
            quartiles: Default::default(),
            smooth: false,
            trendlines: Vec::new(),
        }],
        legend: LegendPos::Bottom,
        data_labels: false,
        data_table: None,
        gridlines: true,
        style: 2,
        hole_size: 75,
        first_slice_angle: 0,
        x_title: None,
        y_title: None,
        source: None,
        by_rows: false,
        pivot: None,
        point_colors: Vec::new(),
        value_axis,
    }
}

#[test]
fn dropdown_settings_round_trip_beside_a_plain_list_rule() {
    let mut wb = Workbook::default();
    let mut s = Sheet::new("Data");
    s.validations.push(Validation {
        ranges: vec![rr("B2:B9")],
        kind: ValidationKind::List,
        f1: "\"Open,In progress,Done\"".into(),
        multiple: true,
        display: DropdownDisplay::Chip,
        item_colors: vec![
            ItemColor { item: "Open".into(), color: Color::Rgb(0xC6DBE1) },
            ItemColor { item: "Done".into(), color: Color::Rgb(0xD4EDBC) },
        ],
        ..Default::default()
    });
    s.validations.push(Validation { ranges: vec![rr("C2:C9")], kind: ValidationKind::List, f1: "\"a,b\"".into(), ..Default::default() });
    s.validations.push(Validation {
        ranges: vec![rr("D2")],
        kind: ValidationKind::List,
        f1: "\"x,y\"".into(),
        display: DropdownDisplay::Plain,
        ..Default::default()
    });
    wb.sheets = vec![Arc::new(s)];
    let bytes = write_xlsx(&wb).unwrap();

    // Excel sees an ordinary list rule (as Google Sheets writes a multiple-selection drop-down)
    // and GridCraft's settings in an extension it skips.
    let mut pkg = Package::open(&bytes).unwrap();
    let xml = String::from_utf8(pkg.read("xl/worksheets/sheet1.xml").unwrap().unwrap()).unwrap();
    assert!(xml.contains("<dataValidation type=\"list\""), "{xml}");
    assert!(!xml.contains("<dataValidation type=\"list\" multiple"), "{xml}");
    let ext = xml.find("{9A3C7E21-5B4D-4F6A-8E2B-1C0D3F5A7B92}").expect("drop-down extension");
    assert!(ext > xml.find("</dataValidations>").unwrap(), "the extension sits in the sheet's extLst, after the rules");
    assert!(!xml.contains("sqref=\"C2:C9\" display"), "a rule with default settings gets no entry: {xml}");

    let (back, _) = read_xlsx(&bytes).unwrap();
    let v = &back.sheets[0].validations;
    assert_eq!(v.len(), 3);
    assert!(v[0].multiple);
    assert_eq!(v[0].display, DropdownDisplay::Chip);
    assert_eq!(v[0].item_color("open"), Some(Color::Rgb(0xC6DBE1)));
    assert_eq!(v[0].item_color("Done"), Some(Color::Rgb(0xD4EDBC)));
    assert_eq!(v[0].item_color("In progress"), None);
    assert!(!v[1].multiple);
    assert_eq!(v[1].display, DropdownDisplay::Arrow);
    assert!(v[1].item_colors.is_empty());
    assert_eq!(v[2].display, DropdownDisplay::Plain);
    assert_eq!(wb.sheets[0].validations, back.sheets[0].validations);
}

#[test]
fn value_axis_bounds_round_trip_in_c_scaling() {
    for kind in [ChartKind::ColumnClustered, ChartKind::BarClustered, ChartKind::Line, ChartKind::Scatter] {
        let mut wb = Workbook::default();
        let mut s = Sheet::new("Data");
        s.charts.push(chart(kind, AxisBounds { min: Some(-5.0), max: Some(250.5) }));
        wb.sheets = vec![Arc::new(s)];
        let bytes = write_xlsx(&wb).unwrap();
        let mut pkg = Package::open(&bytes).unwrap();
        let xml = String::from_utf8(pkg.read("xl/charts/chart1.xml").unwrap().unwrap()).unwrap();
        // Schema order inside c:scaling: orientation, max, min.
        assert!(xml.contains("<c:scaling><c:orientation val=\"minMax\"/><c:max val=\"250.5\"/><c:min val=\"-5\"/></c:scaling>"), "{kind:?}: {xml}");
        assert_eq!(xml.matches("<c:max ").count(), 1, "only the value axis is bounded: {xml}");
        let (back, _) = read_xlsx(&bytes).unwrap();
        assert_eq!(back.sheets[0].charts[0].value_axis, AxisBounds { min: Some(-5.0), max: Some(250.5) }, "{kind:?}");
    }
    // One end only, and none.
    for b in [AxisBounds { min: None, max: Some(10.0) }, AxisBounds::default()] {
        let mut wb = Workbook::default();
        let mut s = Sheet::new("Data");
        s.charts.push(chart(ChartKind::ColumnClustered, b));
        wb.sheets = vec![Arc::new(s)];
        let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
        assert_eq!(back.sheets[0].charts[0].value_axis, b);
    }
}

#[test]
fn bounds_that_leave_nothing_to_plot_are_dropped() {
    assert_eq!(AxisBounds::clean(Some(5.0), Some(5.0)), AxisBounds::default());
    assert_eq!(AxisBounds::clean(Some(f64::NAN), Some(3.0)), AxisBounds { min: None, max: Some(3.0) });
}
