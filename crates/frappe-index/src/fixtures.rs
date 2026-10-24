//! Layer 2 — `<app>/fixtures/*.json` and `<app>/<module>/custom/*.json`.
//!
//! Only two record types matter here: Custom Field appends a field to a DocType
//! and Property Setter overrides one property
//! ([04](../../../docs/specs/04-metadata-index.md)). Every other record kind in
//! a fixtures file is data, not schema, and is skipped.
//!
//! A fixtures file is an array of records, each naming its own `doctype`. A
//! customization file — what Customize Form's "Export Customizations" writes —
//! is one object whose `custom_fields` and `property_setters` arrays hold the
//! records without that key.

use std::path::Path;

use serde_json::Value;

use crate::doctype::{self, RawField};
use crate::span::{self, Range};

/// A `Custom Field` record.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CustomField {
    /// `dt` — the DocType it is added to.
    pub doctype: String,
    /// `insert_after` — the fieldname it follows in `field_order`.
    pub insert_after: Option<String>,
    /// The field itself, shaped exactly like a schema field.
    pub field: RawField,
}

/// A `Property Setter` record.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct PropertySetter {
    /// `doc_type`.
    pub doctype: String,
    /// `field_name`. Empty targets the DocType itself.
    pub field_name: Option<String>,
    /// `property`.
    pub property: String,
    /// `value`, always a string as Frappe stores it.
    pub value: String,
    /// The record range inside the fixtures file.
    pub range: Range,
}

/// Everything one fixtures file contributes.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Fixture {
    pub custom_fields: Vec<CustomField>,
    pub property_setters: Vec<PropertySetter>,
}

impl Fixture {
    /// Every DocType name this file touches.
    pub fn doctypes(&self) -> impl Iterator<Item = &str> {
        self.custom_fields
            .iter()
            .map(|record| record.doctype.as_str())
            .chain(
                self.property_setters
                    .iter()
                    .map(|record| record.doctype.as_str()),
            )
    }
}

/// Read one fixtures file.
///
/// A fixtures file that does not parse contributes nothing and reports why.
/// It is not a `malformed-json` diagnostic: that code is defined for DocType
/// JSON ([23](../../../docs/specs/23-diagnostics.md)), and inventing a second
/// meaning for it would break suppression by code.
pub(crate) fn read(source: &str) -> Result<Fixture, String> {
    let value: Value = serde_json::from_str(source).map_err(|error| error.to_string())?;
    let spans = span::scan(source).ok();
    let mut fixture = Fixture::default();

    if let Some(rows) = value.as_array() {
        let row_spans = spans.as_ref().map(span::Node::elements).unwrap_or_default();
        for (at, row) in rows.iter().enumerate() {
            let kind = row.get("doctype").and_then(Value::as_str);
            fixture.push(kind, row, row_spans.get(at));
        }
        return Ok(fixture);
    }

    if !value.is_object() {
        return Err("neither an array of records nor a customization object".to_string());
    }
    // Frappe syncs a customization file on migrate only when `sync_on_migrate`
    // is set, but also on install of its app regardless, so every file counts.
    for (key, kind) in [
        ("custom_fields", "Custom Field"),
        ("property_setters", "Property Setter"),
    ] {
        let Some(rows) = value.get(key).and_then(Value::as_array) else {
            continue;
        };
        let row_spans = spans
            .as_ref()
            .and_then(|node| node.value(key))
            .map(span::Node::elements)
            .unwrap_or_default();
        for (at, row) in rows.iter().enumerate() {
            fixture.push(Some(kind), row, row_spans.get(at));
        }
    }
    Ok(fixture)
}

impl Fixture {
    /// Add one record of `kind`; anything else, or a record missing its
    /// target, is skipped.
    fn push(&mut self, kind: Option<&str>, row: &Value, node: Option<&span::Node>) {
        let Some(object) = row.as_object() else {
            return;
        };
        match kind {
            Some("Custom Field") => {
                let Some(doctype) = doctype::string(object, "dt") else {
                    return;
                };
                let Some(field) = doctype::parse_field(row, node) else {
                    return;
                };
                self.custom_fields.push(CustomField {
                    doctype,
                    insert_after: doctype::string(object, "insert_after"),
                    field,
                });
            }
            Some("Property Setter") => {
                let Some(doctype) = doctype::string(object, "doc_type") else {
                    return;
                };
                let Some(property) = doctype::string(object, "property") else {
                    return;
                };
                self.property_setters.push(PropertySetter {
                    doctype,
                    field_name: doctype::string(object, "field_name"),
                    property,
                    value: object.get("value").map(value_as_string).unwrap_or_default(),
                    range: node.map_or_else(Range::none, |node| node.range),
                });
            }
            _ => {}
        }
    }
}

/// Property Setter `value` is a string column, but exported fixtures carry
/// numbers and booleans unquoted often enough to matter.
fn value_as_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Whether a path is a Layer 2 file: `<package>/fixtures/*.json` or
/// `<package>/<module>/custom/*.json`.
#[must_use]
pub fn is_fixture(package: &Path, path: &Path) -> bool {
    if !path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        return false;
    }
    let Some(relative) = path.strip_prefix(package).ok().and_then(Path::parent) else {
        return false;
    };
    let directories: Vec<_> = relative.components().map(|part| part.as_os_str()).collect();
    // Frappe reads `custom/` only directly under a module, and modules sit
    // directly under the package.
    matches!(directories.as_slice(), [only] if *only == "fixtures")
        || matches!(directories.as_slice(), [_, custom] if *custom == "custom")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn bench() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repo root")
            .join("tests/fixtures/bench")
    }

    #[test]
    fn custom_fields_read_as_fields() {
        let source = std::fs::read_to_string(
            bench().join("apps/library/library/fixtures/custom_field.json"),
        )
        .expect("readable fixture");
        let fixture = read(&source).expect("parses");

        assert_eq!(fixture.custom_fields.len(), 2);
        assert!(fixture.property_setters.is_empty());

        let loyalty = &fixture.custom_fields[0];
        assert_eq!(loyalty.doctype, "Library Member");
        assert_eq!(loyalty.field.fieldname, "loyalty_points");
        assert_eq!(loyalty.field.fieldtype, "Int");
        assert_eq!(loyalty.insert_after.as_deref(), Some("loans"));
        assert_eq!(
            loyalty
                .field
                .range
                .text(&source)
                .map(|text| text.starts_with('{')),
            Some(true)
        );
    }

    #[test]
    fn property_setters_keep_their_target() {
        let source = std::fs::read_to_string(
            bench().join("apps/library/library/fixtures/property_setter.json"),
        )
        .expect("readable fixture");
        let fixture = read(&source).expect("parses");

        assert_eq!(fixture.property_setters.len(), 2);

        let field_level = &fixture.property_setters[0];
        assert_eq!(field_level.doctype, "Library Member");
        assert_eq!(field_level.field_name.as_deref(), Some("email_id"));
        assert_eq!(field_level.property, "reqd");
        assert_eq!(field_level.value, "1");

        // An empty `field_name` targets the DocType, not a field.
        let document_level = &fixture.property_setters[1];
        assert_eq!(document_level.field_name, None);
        assert_eq!(document_level.property, "search_fields");
        assert_eq!(document_level.value, "member_name,email_id");
    }

    #[test]
    fn a_customization_file_reads_its_records_without_their_doctype_key() {
        let source = std::fs::read_to_string(
            bench().join("apps/circulation-app/circulation/circulation/custom/library_member.json"),
        )
        .expect("readable customization");
        let fixture = read(&source).expect("parses");

        assert_eq!(fixture.custom_fields.len(), 1);
        let tier = &fixture.custom_fields[0];
        assert_eq!(tier.doctype, "Library Member");
        assert_eq!(tier.field.fieldname, "circulation_tier");
        assert_eq!(
            tier.field
                .range
                .text(&source)
                .map(|text| text.starts_with('{')),
            Some(true)
        );

        assert_eq!(fixture.property_setters.len(), 1);
        assert_eq!(fixture.property_setters[0].property, "default");
        assert_eq!(fixture.property_setters[0].value, "Standard");
    }

    #[test]
    fn a_customization_file_not_synced_on_migrate_still_counts() {
        // Frappe applies it when its app is installed.
        let fixture = read(
            r#"{"doctype": "Address", "sync_on_migrate": 0,
                "custom_fields": [{"dt": "Address", "fieldname": "gstin", "fieldtype": "Data"}]}"#,
        )
        .expect("parses");
        assert_eq!(fixture.custom_fields.len(), 1);
    }

    #[test]
    fn other_record_kinds_in_a_fixtures_file_are_skipped() {
        let fixture = read(r#"[{"doctype": "Library Member", "name": "x"}]"#).expect("parses");
        assert_eq!(fixture, Fixture::default());
    }

    #[test]
    fn a_fixtures_file_that_is_not_an_array_reports_rather_than_panics() {
        assert!(read("1").is_err());
        assert!(read("not json").is_err());
    }

    #[test]
    fn only_files_directly_under_fixtures_are_layer_two() {
        let package = bench().join("apps/library/library");
        assert!(is_fixture(
            &package,
            &package.join("fixtures/custom_field.json")
        ));
        assert!(!is_fixture(
            &package,
            &package.join("fixtures/nested/x.json")
        ));
        assert!(!is_fixture(&package, &package.join("api.py")));
    }

    #[test]
    fn only_custom_directly_under_a_module_is_layer_two() {
        let package = bench().join("apps/library/library");
        assert!(is_fixture(&package, &package.join("library/custom/x.json")));
        assert!(!is_fixture(&package, &package.join("custom/x.json")));
        assert!(!is_fixture(
            &package,
            &package.join("library/doctype/custom/x.json")
        ));
    }
}
