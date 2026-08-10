//! Fieldtype tables and implicit fields, copied out of `frappe`.
//!
//! Source of truth is `frappe/model/__init__.py`, snapshotted at
//! [`SNAPSHOT_VERSION`]. Everything here is version-gated by
//! [31](../../../docs/specs/31-performance.md): on a bench whose Frappe version
//! does not match exactly, an unrecognized value degrades to "unknown" and no
//! diagnostic is emitted.
//!
//! These are hand-vendored. `cargo xtask codegen` owns generating them from a
//! checkout, and lands with its own task.

/// The `frappe` `__version__` these tables were copied from.
pub const SNAPSHOT_VERSION: &str = "17.0.0-dev";

/// `data_fieldtypes` — fieldtypes that hold a value in a column.
pub const DATA_FIELDTYPES: [&str; 34] = [
    "Currency",
    "Int",
    "Long Int",
    "Float",
    "Percent",
    "Check",
    "Small Text",
    "Long Text",
    "Code",
    "Text Editor",
    "Markdown Editor",
    "HTML Editor",
    "Date",
    "Datetime",
    "Time",
    "Text",
    "Data",
    "Link",
    "Dynamic Link",
    "Password",
    "Select",
    "Rating",
    "Read Only",
    "Attach",
    "Attach Image",
    "Signature",
    "Color",
    "Barcode",
    "Geolocation",
    "Duration",
    "Icon",
    "Phone",
    "Autocomplete",
    "JSON",
];

/// `no_value_fields` — layout and child-table fieldtypes with no column.
pub const NO_VALUE_FIELDS: [&str; 10] = [
    "Section Break",
    "Column Break",
    "Tab Break",
    "HTML",
    "Table",
    "Table MultiSelect",
    "Button",
    "Image",
    "Fold",
    "Heading",
];

/// `display_fieldtypes` — the layout subset of [`NO_VALUE_FIELDS`].
pub const DISPLAY_FIELDTYPES: [&str; 8] = [
    "Section Break",
    "Column Break",
    "Tab Break",
    "HTML",
    "Button",
    "Image",
    "Fold",
    "Heading",
];

/// `table_fields` — the two fieldtypes whose `options` names a child DocType.
pub const TABLE_FIELDS: [&str; 2] = ["Table", "Table MultiSelect"];

/// `numeric_fieldtypes`.
pub const NUMERIC_FIELDTYPES: [&str; 6] =
    ["Currency", "Int", "Long Int", "Float", "Percent", "Check"];

/// `attachment_fieldtypes`.
pub const ATTACHMENT_FIELDTYPES: [&str; 2] = ["Attach", "Attach Image"];

/// `data_field_options` — the format hints a `Data` field's `options` may hold.
pub const DATA_FIELD_OPTIONS: [&str; 6] = ["Email", "Name", "Phone", "URL", "Barcode", "IBAN"];

/// Every fieldtype Frappe knows: [`DATA_FIELDTYPES`] plus [`NO_VALUE_FIELDS`].
#[must_use]
pub fn is_known(fieldtype: &str) -> bool {
    DATA_FIELDTYPES.contains(&fieldtype) || NO_VALUE_FIELDS.contains(&fieldtype)
}

/// Whether `fieldtype` holds a value in a column.
#[must_use]
pub fn has_value(fieldtype: &str) -> bool {
    DATA_FIELDTYPES.contains(&fieldtype)
}

/// Whether `fieldtype`'s `options` names a child DocType.
#[must_use]
pub fn is_table(fieldtype: &str) -> bool {
    TABLE_FIELDS.contains(&fieldtype)
}

/// An implicit field: present on every document, absent from the JSON.
///
/// [04](../../../docs/specs/04-metadata-index.md) requires these to be
/// completable and never reported as unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Implicit {
    /// The fieldname.
    pub fieldname: &'static str,
    /// The fieldtype the column has, for hover and completion detail.
    pub fieldtype: &'static str,
    /// `options` where the column is a `Link`.
    pub options: Option<&'static str>,
    /// Which table it comes from.
    pub group: Group,
}

/// Which of `frappe/model/__init__.py`'s tables an implicit field comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// `default_fields` — on every DocType.
    Standard,
    /// `child_table_fields` — on child DocTypes only.
    Child,
    /// `optional_fields` — the `_`-prefixed meta columns.
    Optional,
}

/// `default_fields`.
///
/// Frappe's tuple leads with `doctype`, which
/// [04](../../../docs/specs/04-metadata-index.md) omits; `doc.doctype` is valid
/// on every document, so leaving it out would make a correct expression report
/// as unknown.
pub const STANDARD: [Implicit; 8] = [
    Implicit {
        fieldname: "doctype",
        fieldtype: "Data",
        options: None,
        group: Group::Standard,
    },
    Implicit {
        fieldname: "name",
        fieldtype: "Data",
        options: None,
        group: Group::Standard,
    },
    Implicit {
        fieldname: "owner",
        fieldtype: "Link",
        options: Some("User"),
        group: Group::Standard,
    },
    Implicit {
        fieldname: "creation",
        fieldtype: "Datetime",
        options: None,
        group: Group::Standard,
    },
    Implicit {
        fieldname: "modified",
        fieldtype: "Datetime",
        options: None,
        group: Group::Standard,
    },
    Implicit {
        fieldname: "modified_by",
        fieldtype: "Link",
        options: Some("User"),
        group: Group::Standard,
    },
    Implicit {
        fieldname: "docstatus",
        fieldtype: "Int",
        options: None,
        group: Group::Standard,
    },
    Implicit {
        fieldname: "idx",
        fieldtype: "Int",
        options: None,
        group: Group::Standard,
    },
];

/// `child_table_fields`.
pub const CHILD: [Implicit; 3] = [
    Implicit {
        fieldname: "parent",
        fieldtype: "Data",
        options: None,
        group: Group::Child,
    },
    Implicit {
        fieldname: "parentfield",
        fieldtype: "Data",
        options: None,
        group: Group::Child,
    },
    Implicit {
        fieldname: "parenttype",
        fieldtype: "Link",
        options: Some("DocType"),
        group: Group::Child,
    },
];

/// `optional_fields`.
pub const OPTIONAL: [Implicit; 5] = [
    Implicit {
        fieldname: "_user_tags",
        fieldtype: "Text",
        options: None,
        group: Group::Optional,
    },
    Implicit {
        fieldname: "_comments",
        fieldtype: "Text",
        options: None,
        group: Group::Optional,
    },
    Implicit {
        fieldname: "_assign",
        fieldtype: "Text",
        options: None,
        group: Group::Optional,
    },
    Implicit {
        fieldname: "_liked_by",
        fieldtype: "Text",
        options: None,
        group: Group::Optional,
    },
    Implicit {
        fieldname: "_seen",
        fieldtype: "Text",
        options: None,
        group: Group::Optional,
    },
];

/// Every implicit field of a DocType, child fields included only when it is one.
pub fn implicit(is_child: bool) -> impl Iterator<Item = &'static Implicit> {
    STANDARD
        .iter()
        .chain(if is_child { CHILD.as_slice() } else { &[] })
        .chain(OPTIONAL.iter())
}

/// Whether `fieldname` is implicit on a DocType of this shape.
#[must_use]
pub fn is_implicit(fieldname: &str, is_child: bool) -> bool {
    implicit(is_child).any(|field| field.fieldname == fieldname)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_match_frappes_own() {
        assert!(is_known("Dynamic Link"));
        assert!(is_known("Table MultiSelect"));
        assert!(!is_known("Dynamic-Link"));
        assert!(!is_known(""));

        assert!(has_value("Data"));
        assert!(!has_value("Section Break"), "layout fields hold no value");
        assert!(is_table("Table MultiSelect"));
        assert!(!is_table("Link"));
    }

    #[test]
    fn a_child_doctype_gains_the_parent_trio_and_nothing_else_does() {
        assert!(is_implicit("parentfield", true));
        assert!(!is_implicit("parentfield", false));

        for fieldname in ["name", "owner", "creation", "docstatus", "idx", "doctype"] {
            assert!(is_implicit(fieldname, false), "{fieldname}");
        }
        for fieldname in ["_user_tags", "_comments", "_assign", "_liked_by", "_seen"] {
            assert!(is_implicit(fieldname, false), "{fieldname}");
        }
    }

    #[test]
    fn a_declared_fieldname_is_not_implicit() {
        assert!(!is_implicit("member_name", true));
        assert!(!is_implicit("parent_name", true), "prefix, not the field");
    }

    #[test]
    fn the_counts_are_the_ones_spec_04_lists() {
        assert_eq!(implicit(false).count(), 13, "8 standard + 5 optional");
        assert_eq!(implicit(true).count(), 16, "plus the child trio");
    }
}
