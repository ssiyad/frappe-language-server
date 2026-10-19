//! What the bridge reads — [05](../../../docs/specs/05-live-bridge.md).
//!
//! Custom Field and Property Setter arrive in the same shape they have in a
//! fixtures file, which is what lets Layer 3 reuse Layer 2's merge
//! ([04](../../../docs/specs/04-metadata-index.md)) rather than growing a
//! second one that would eventually disagree with it.

use serde_json::{Map, Value};

use crate::client::{Client, Error};

/// One site's customizations, as records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sync {
    /// `Custom Field` rows.
    pub custom_fields: Vec<Value>,
    /// `Property Setter` rows.
    pub property_setters: Vec<Value>,
    /// DocTypes created through the UI, which are on no disk.
    pub custom_doctypes: Vec<Value>,
}

impl Sync {
    /// How many records were read, for the log line and the status report.
    #[must_use]
    pub fn len(&self) -> usize {
        self.custom_fields.len() + self.property_setters.len() + self.custom_doctypes.len()
    }

    /// Whether the site had no customizations at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The fields a Custom Field record needs to merge as a schema field.
const CUSTOM_FIELD: [&str; 14] = [
    "name",
    "dt",
    "fieldname",
    "fieldtype",
    "label",
    "options",
    "default",
    "reqd",
    "hidden",
    "read_only",
    "unique",
    "permlevel",
    "depends_on",
    "fetch_from",
];

const PROPERTY_SETTER: [&str; 6] = [
    "name",
    "doc_type",
    "field_name",
    "property",
    "value",
    "property_type",
];

const CUSTOM_DOCTYPE: [&str; 6] = [
    "name",
    "module",
    "istable",
    "issingle",
    "is_submittable",
    "custom",
];

/// Read one site's Layer 3.
///
/// # Errors
///
/// Any transport failure. A partial read is not returned: half a picture
/// merged as if it were whole is worse than none, because the missing half
/// looks like absence ([23](../../../docs/specs/23-diagnostics.md)).
pub async fn read(client: &Client) -> Result<Sync, Error> {
    let custom_fields = client.list("Custom Field", &CUSTOM_FIELD, None).await?;
    let property_setters = client
        .list("Property Setter", &PROPERTY_SETTER, None)
        .await?;
    let custom_doctypes = client
        .list("DocType", &CUSTOM_DOCTYPE, Some(r#"[["custom","=",1]]"#))
        .await?;

    Ok(Sync {
        custom_fields: rows(custom_fields),
        property_setters: rows(property_setters),
        custom_doctypes: rows(custom_doctypes),
    })
}

/// Keep only object rows. A list call cannot return anything else, but the
/// site is not this process and the merge below assumes objects.
fn rows(values: Vec<Value>) -> Vec<Value> {
    values
        .into_iter()
        .filter(|value| matches!(value, Value::Object(_)))
        .collect()
}

/// The records as a fixtures-file body, which is what Layer 2's reader parses.
///
/// The shapes are identical: `Custom Field` and `Property Setter` are ordinary
/// DocTypes, and a fixtures file is a dump of their rows. Reusing the reader
/// means Layer 3 cannot drift from Layer 2 in how it reads a `depends_on`.
#[must_use]
pub fn as_fixture_body(records: &[Value], doctype: &str) -> String {
    let tagged: Vec<Value> = records
        .iter()
        .map(|record| {
            let mut row: Map<String, Value> = record.as_object().cloned().unwrap_or_default();
            row.insert("doctype".to_string(), Value::String(doctype.to_string()));
            Value::Object(row)
        })
        .collect();
    serde_json::to_string(&tagged).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_become_a_fixture_body_the_layer_two_reader_accepts() {
        let records = vec![serde_json::json!({
            "name": "Item-custom_note",
            "dt": "Item",
            "fieldname": "custom_note",
            "fieldtype": "Small Text",
        })];

        let body = as_fixture_body(&records, "Custom Field");
        let parsed: Vec<Value> = serde_json::from_str(&body).expect("valid JSON");
        // The `doctype` tag is what the fixtures reader dispatches on, and a
        // list call does not return it.
        assert_eq!(parsed[0]["doctype"], "Custom Field");
        assert_eq!(parsed[0]["fieldname"], "custom_note");
    }

    #[test]
    fn a_non_object_row_is_dropped() {
        let kept = rows(vec![serde_json::json!({"a": 1}), serde_json::json!("nope")]);
        assert_eq!(kept.len(), 1);
    }
}
