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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// Attributes and methods every `Document` carries, which are not fields.
///
/// `doc.flags`, `doc.save`, `doc.meta` are ordinary Python attribute access on
/// the controller class. Without this list they read as fieldnames, and
/// `unknown-field` fires on them across `frappe` itself — 24 DocTypes for
/// `flags` alone, which is precisely the false positive
/// [23](../../../docs/specs/23-diagnostics.md) forbids.
///
/// Vendored from `Document` and `BaseDocument` in `frappe/model/document.py`
/// and `frappe/model/base_document.py` at frappe `5a94ce8576`. Names that are also
/// implicit fields are excluded: those are fields first.
pub const DOCUMENT_MEMBERS: [&str; 165] = [
    "_DOCTYPE_NAME",
    "_cancel",
    "_evaluate_virtual_field_options",
    "_extract_images_from_text_editor",
    "_fix_numeric_types",
    "_fix_rating_value",
    "_get_missing_mandatory_fields",
    "_get_table_fields",
    "_handle_hash_conflict",
    "_handle_permission_failure",
    "_init_child",
    "_load_child_table_from_db",
    "_non_computed_table_fieldnames",
    "_parent_doc",
    "_precision",
    "_prefetch_link_values",
    "_rename",
    "_restore_masked_fields_from_db",
    "_sanitize_content",
    "_save",
    "_save_passwords",
    "_set_defaults",
    "_submit",
    "_sync_autoname_field",
    "_table_fieldnames",
    "_validate",
    "_validate_code_fields",
    "_validate_constants",
    "_validate_data_fields",
    "_validate_length",
    "_validate_links",
    "_validate_mandatory",
    "_validate_non_negative",
    "_validate_selects",
    "_validate_update_after_submit",
    "_weakref",
    "add_comment",
    "add_seen",
    "add_tag",
    "add_viewed",
    "append",
    "apply_fieldlevel_read_permissions",
    "as_dict",
    "as_json",
    "cancel",
    "cast",
    "check_docstatus_transition",
    "check_if_latest",
    "check_if_locked",
    "check_no_back_links_exist",
    "check_permission",
    "clear_cache",
    "copy_attachments_from_amended_from",
    "db_get",
    "db_insert",
    "db_set",
    "db_update",
    "db_update_all",
    "deferred_insert",
    "delete",
    "delete_key",
    "discard",
    "docs",
    "dont_update_if_missing",
    "extend",
    "flags",
    "get",
    "get_all_children",
    "get_assigned_users",
    "get_db_value",
    "get_doc_before_save",
    "get_document_share_key",
    "get_field_name_by_key_name",
    "get_formatted",
    "get_invalid_links",
    "get_label_from_fieldname",
    "get_latest",
    "get_liked_by",
    "get_onload",
    "get_parentfield_of_doctype",
    "get_password",
    "get_permissions",
    "get_permlevel_access",
    "get_signature",
    "get_table_field_doctype",
    "get_tags",
    "get_title",
    "get_url",
    "get_valid_columns",
    "get_valid_dict",
    "get_value",
    "get_value_before_save",
    "get_virtual_field_value",
    "getone",
    "has_permission",
    "has_permlevel_access_to",
    "has_value_changed",
    "hook",
    "in_format_data",
    "init_child_tables",
    "init_valid_columns",
    "insert",
    "is_child_table_same",
    "is_dummy_password",
    "is_locked",
    "is_new",
    "is_print_hide",
    "is_whitelisted",
    "load_children_from_db",
    "load_doc_before_save",
    "load_from_db",
    "lock",
    "log_error",
    "mask_fields",
    "meta",
    "notify_update",
    "parent_doc",
    "permitted_fieldnames",
    "precision",
    "print_hide",
    "queue_action",
    "raise_no_permission_to",
    "reload",
    "remove",
    "remove_tag",
    "remove_unpicklable_values",
    "rename",
    "reset_computed_child_tables",
    "reset_seen",
    "reset_values_if_no_permlevel_access",
    "round_floats_in",
    "run_before_save_methods",
    "run_method",
    "run_notifications",
    "run_post_save_methods",
    "run_trigger",
    "save",
    "save_version",
    "set",
    "set_docstatus",
    "set_fetch_from_value",
    "set_name_in_children",
    "set_new_name",
    "set_onload",
    "set_parent_in_children",
    "set_title_field",
    "set_user_and_timestamp",
    "show_unique_validation_message",
    "submit",
    "throw_length_exceeded_error",
    "unlock",
    "update",
    "update_child_table",
    "update_children",
    "update_if_missing",
    "update_modified",
    "update_single",
    "validate_amended_from",
    "validate_from_to_dates",
    "validate_higher_perm_levels",
    "validate_set_only_once",
    "validate_table_has_rows",
    "validate_update_after_submit",
    "validate_value",
    "validate_workflow",
];

/// Tables frappe queries by name that are not DocTypes.
///
/// `frappe.db.delete("Singles", …)` and `frappe.db.sql("… from tabSeries")`
/// are ordinary Frappe. None of these has a DocType JSON anywhere, and every
/// one of them was reported as a DocType nobody has
/// ([32](../../../docs/specs/32-testing.md)). Sorted for binary search.
pub const RAW_TABLES: [&str; 8] = [
    "Series",
    "Sessions",
    "Singles",
    "__Auth",
    "__UserSettings",
    "__global_search",
    "tabSeries",
    "tabSingles",
];

/// Whether a name is one of frappe's own tables rather than a DocType.
#[must_use]
pub fn is_raw_table(name: &str) -> bool {
    RAW_TABLES.binary_search(&name).is_ok()
}

/// Whether attribute access names a `Document` member rather than a field.
///
/// Dunders are Python's own and never fieldnames; a single underscore is not
/// enough, because `_user_tags` and friends are real optional fields
/// ([04](../../../docs/specs/04-metadata-index.md)).
#[must_use]
pub fn is_document_member(name: &str) -> bool {
    name.starts_with("__") || DOCUMENT_MEMBERS.binary_search(&name).is_ok()
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
