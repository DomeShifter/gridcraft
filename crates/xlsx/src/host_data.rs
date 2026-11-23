//! Host data (see `Workbook::host_data`) in a file: a custom XML part, the way Office add-ins keep
//! their data, which Excel and LibreOffice keep through their own saves but don't show (StarSheets).
//!
//! `customXml/itemN.xml` = `<hostData xmlns="urn:gridcraft:host-data:1">text</hostData>`, with an
//! `itemProps` part, related from the workbook. On read it is taken out of the kept (preserved)
//! parts into `Workbook::host_data`; on write it is added back.

use std::sync::Arc;

use gridcraft_model::{Preserved, PreservedPart, PreservedRel, Workbook};

/// The part's namespace: how the reader recognises it.
pub const NS: &str = "urn:gridcraft:host-data:1";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PROPS_TYPE: &str = "application/vnd.openxmlformats-officedocument.customXmlProperties+xml";

fn body(text: &str) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<hostData xmlns=\"{NS}\">{}</hostData>", crate::xml::esc(text))
}

/// The text of a host data part, or `None` if `data` isn't one.
fn text_of(data: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(data).ok()?;
    let start = s.find("<hostData")?;
    if !s[start..].split('>').next()?.contains(NS) {
        return None;
    }
    let open_end = start + s[start..].find('>')? + 1;
    if s[..open_end].ends_with("/>") {
        return Some(String::new());
    }
    let close = s.rfind("</hostData>")?;
    let inner = s.get(open_end..close)?;
    quick_xml::escape::unescape(inner).ok().map(|t| t.into_owned())
}

/// Takes the host data part (and its properties part and relationship) out of `kept`.
pub(crate) fn take(kept: &mut Option<Arc<Preserved>>) -> Option<String> {
    let p = kept.as_ref()?;
    let (i, text) = p.parts.iter().enumerate().find_map(|(i, part)| text_of(&part.data).map(|t| (i, t)))?;
    let mut p = (**p).clone();
    let part = p.parts.remove(i);
    let props: Vec<String> = part.rels.iter().map(|r| r.target.clone()).collect();
    p.parts.retain(|x| !props.contains(&x.name));
    p.rels.retain(|r| r.target != part.name);
    p.package_rels.retain(|r| r.target != part.name);
    *kept = if p.is_empty() { None } else { Some(Arc::new(p)) };
    Some(text)
}

/// `wb` with its host data as a kept custom XML part, ready to write (`None`: nothing to add).
pub(crate) fn with_part(wb: &Workbook) -> Option<Workbook> {
    let text = wb.host_data.as_ref()?;
    let mut out = wb.clone();
    let mut p = out.preserved.as_deref().cloned().unwrap_or_default();
    let used = |n: &str| p.parts.iter().any(|x| x.name.eq_ignore_ascii_case(n));
    let n = (1..).find(|n| !used(&format!("customXml/item{n}.xml")) && !used(&format!("customXml/itemProps{n}.xml"))).unwrap_or(1);
    let (item, props) = (format!("customXml/item{n}.xml"), format!("customXml/itemProps{n}.xml"));
    let props_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<ds:datastoreItem ds:itemID=\"{{6A1F5D2E-3B7C-4E8A-9D10-484F53544441}}\" xmlns:ds=\"http://schemas.openxmlformats.org/officeDocument/2006/customXml\"><ds:schemaRefs><ds:schemaRef ds:uri=\"{NS}\"/></ds:schemaRefs></ds:datastoreItem>"
    );
    p.rels.push(PreservedRel { id: "rIdHostData".into(), rel_type: format!("{REL}/customXml"), target: item.clone(), external: false });
    p.parts.push(PreservedPart {
        name: item,
        content_type: "application/xml".into(),
        data: body(text).into_bytes().into(),
        rels: vec![PreservedRel { id: "rId1".into(), rel_type: format!("{REL}/customXmlProps"), target: props.clone(), external: false }],
    });
    p.parts.push(PreservedPart { name: props, content_type: PROPS_TYPE.into(), data: props_xml.into_bytes().into(), rels: vec![] });
    out.preserved = Some(Arc::new(p));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_data_round_trips_through_a_file_and_an_edit() {
        let text = r#"{"v":1,"note":"a <b> & \"c\" ]]> d"}"#;
        let mut wb = Workbook::new();
        wb.host_data = Some(text.to_string());
        let bytes = crate::write_xlsx(&wb).unwrap();
        let (mut back, _) = crate::read_xlsx(&bytes).unwrap();
        assert_eq!(back.host_data.as_deref(), Some(text));
        // Not also kept as an unmodelled part (it would be written twice).
        assert!(back.preserved.as_ref().is_none_or(|p| p.parts.iter().all(|x| text_of(&x.data).is_none())));
        back.host_data = Some("second".into());
        let (again, _) = crate::read_xlsx(&crate::write_xlsx(&back).unwrap()).unwrap();
        assert_eq!(again.host_data.as_deref(), Some("second"));
        // No host data: no part.
        let (none, _) = crate::read_xlsx(&crate::write_xlsx(&Workbook::new()).unwrap()).unwrap();
        assert_eq!(none.host_data, None);
    }

    #[test]
    fn a_file_saved_elsewhere_keeps_it() {
        // LibreOffice writes the part back with a line break after the declaration.
        let data = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<hostData xmlns=\"{NS}\">x &amp; y</hostData>");
        assert_eq!(text_of(data.as_bytes()).as_deref(), Some("x & y"));
        assert_eq!(text_of(b"<other xmlns=\"x\">no</other>"), None);
    }
}
