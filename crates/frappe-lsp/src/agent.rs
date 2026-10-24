//! Structured answers for programs rather than editors —
//! [30](../../../docs/specs/30-configuration.md)'s custom methods.
//!
//! Hover is markdown for a person. A coding agent asking "what fields does
//! Sales Invoice have" needs the schema as data, and "who uses this field" by
//! name rather than by a cursor position in an open buffer.

use std::collections::BTreeMap;
use std::path::Path;

use frappe_index::Index;
use frappe_index::occurrences::Key;
use serde_json::{Value, json};

/// How many reference sites one answer carries. The total is always given.
const MAX_SITES: usize = 500;

/// `frappe-lsp/describe` — one DocType's schema.
#[must_use]
pub fn describe(index: &Index, name: &str) -> Value {
    let Some(doctype) = index.doctype(name) else {
        return json!({
            "found": false,
            "name": name,
            "similar": index.case_insensitive_matches(name),
        });
    };

    let fields: Vec<Value> = doctype
        .fields
        .iter()
        .map(|field| {
            json!({
                "fieldname": index.fieldname(field),
                "fieldtype": index.fieldtype(field),
                "label": field.label,
                "options": field.options,
                "reqd": field.reqd,
                "hidden": field.hidden,
                "read_only": field.read_only,
                "layer": field.provenance.layer.as_str(),
            })
        })
        .collect();
    let parents: Vec<&str> = index
        .parents(name)
        .map(|parent| parent.name.as_str())
        .collect();
    let scripts: Vec<Value> = index
        .site_scripts_for(name)
        .map(|script| {
            json!({
                "kind": format!("{:?}", script.kind),
                "name": script.name,
                "trigger": script.trigger,
                "enabled": script.enabled,
                "path": script.path,
            })
        })
        .collect();

    json!({
        "found": true,
        "name": doctype.name,
        "module": doctype.module,
        "app": doctype.app.as_ref(),
        "createdThroughTheUi": doctype.is_site_only(),
        "path": (!doctype.is_site_only()).then(|| doctype.path.to_path_buf()),
        "controller": doctype.controller.as_ref().map(|controller| json!({
            "path": controller.path,
            "module": controller.module,
            "class": controller.class_name,
        })),
        "isChild": doctype.is_child,
        "isSingle": doctype.is_single,
        "isSubmittable": doctype.is_submittable,
        "isVirtual": doctype.is_virtual,
        "isTree": doctype.is_tree,
        "autoname": doctype.autoname,
        "titleField": doctype.title_field,
        "parents": parents,
        "fields": fields,
        "scripts": scripts,
    })
}

/// What a reference list holds and what it cannot, said with every answer: an
/// agent that reads the list as complete renames half a field.
pub const REFERENCES_COVER: &str = "Covers Python, desk JavaScript, DocType JSON, hooks.py, \
fixtures and templates inside app packages. Not covered: code outside an app package, such as a \
frappe-ui dashboard, and attribute reads on a variable not tied to a DocType. Before a rename, \
confirm with grep there.";

/// `frappe-lsp/references` — every site that names a DocType, or one of its
/// fields, as a path, a 1-based line and the line itself.
#[must_use]
pub fn references(index: &Index, doctype: &str, fieldname: Option<&str>) -> Value {
    if !index.occurrences().scanned {
        return json!({"ready": false, "total": 0, "sites": []});
    }
    let key = match fieldname {
        Some(fieldname) => Key::Field(doctype.to_string(), fieldname.to_string()),
        None => Key::DocType(doctype.to_string()),
    };
    let sites = index.occurrences().get(&key);

    let mut by_file: BTreeMap<&Path, Vec<usize>> = BTreeMap::new();
    for site in sites.iter().take(MAX_SITES) {
        by_file
            .entry(site.path.as_ref())
            .or_default()
            .push(site.range.start as usize);
    }
    let mut out = Vec::new();
    for (path, offsets) in by_file {
        let source = std::fs::read_to_string(path).unwrap_or_default();
        for offset in offsets {
            let offset = offset.min(source.len());
            let line_start = source[..offset].rfind('\n').map_or(0, |at| at + 1);
            let line_end = source[offset..]
                .find('\n')
                .map_or(source.len(), |at| offset + at);
            out.push(json!({
                "path": path,
                "line": source[..offset].matches('\n').count() + 1,
                "text": source[line_start..line_end].trim(),
            }));
        }
    }
    json!({"ready": true, "total": sites.len(), "sites": out, "covers": REFERENCES_COVER})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Index {
        let mut index = Index::build(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .expect("repo root")
                .join("tests/fixtures/bench"),
        );
        index.scan_occurrences();
        index
    }

    #[test]
    fn a_doctype_is_described_as_data() {
        let index = index();
        let member = describe(&index, "Library Member");
        assert_eq!(member["found"], true);
        assert_eq!(member["controller"]["class"], "LibraryMember");
        let fields = member["fields"].as_array().expect("fields");
        assert!(
            fields
                .iter()
                .any(|field| field["fieldname"] == "member_name")
        );
        // A fixture's Custom Field says where it came from.
        assert!(fields.iter().any(|field| field["layer"] == "fixture"));

        let item = describe(&index, "Library Loan Item");
        assert_eq!(item["isChild"], true);
        assert!(
            item["parents"]
                .as_array()
                .expect("parents")
                .contains(&json!("Library Member"))
        );
    }

    #[test]
    fn an_unknown_name_offers_its_near_spellings() {
        let missing = describe(&index(), "library member");
        assert_eq!(missing["found"], false);
        assert!(
            missing["similar"]
                .as_array()
                .expect("similar")
                .contains(&json!("Library Member"))
        );
    }

    #[test]
    fn references_by_name_give_lines() {
        let found = references(&index(), "Library Member", None);
        assert_eq!(found["ready"], true);
        let sites = found["sites"].as_array().expect("sites");
        assert!(!sites.is_empty());
        assert!(
            sites
                .iter()
                .all(|site| site["text"].as_str().is_some_and(|text| !text.is_empty()))
        );
    }

    #[test]
    fn a_field_is_referenced_from_field_order_too() {
        let found = references(&index(), "Library Member", Some("email_id"));
        let json_lines: Vec<&str> = found["sites"]
            .as_array()
            .expect("sites")
            .iter()
            .filter(|site| {
                site["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("library_member.json"))
            })
            .filter_map(|site| site["text"].as_str())
            .collect();
        // The declaration, and the `field_order` entry.
        assert!(
            json_lines.contains(&"\"fieldname\": \"email_id\","),
            "{json_lines:?}"
        );
        assert!(json_lines.contains(&"\"email_id\","), "{json_lines:?}");
    }

    #[test]
    fn a_field_is_referenced_from_its_generated_type_annotation() {
        let found = references(&index(), "Library Membership", Some("member"));
        let annotated = found["sites"]
            .as_array()
            .expect("sites")
            .iter()
            .any(|site| {
                site["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("library_membership.py"))
                    && site["text"] == "member: DF.Link"
            });
        assert!(annotated, "{found}");
    }

    #[test]
    fn a_field_is_referenced_from_a_query_row() {
        let found = references(&index(), "Library Member", Some("email_id"));
        let read = found["sites"]
            .as_array()
            .expect("sites")
            .iter()
            .any(|site| {
                site["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("circulation/api.py"))
                    && site["text"] == "return member.email_id  # expect: clean"
            });
        assert!(read, "{found}");
    }

    #[test]
    fn references_say_what_they_do_not_cover() {
        let found = references(&index(), "Library Member", Some("email_id"));
        assert_eq!(found["covers"], REFERENCES_COVER);
    }
}
