//! The entities of [04](../../../docs/specs/04-metadata-index.md) and the
//! parser that reads a DocType JSON into them.
//!
//! Parsing is split in two: [`parse`] runs in parallel and yields owned strings,
//! and the merge in [`crate::index`] interns them. Interning needs a single
//! writer, and the merge is single-threaded anyway
//! ([31](../../../docs/specs/31-performance.md)).

use std::path::Path;
use std::sync::Arc;

use serde_json::Value;

use crate::fieldtypes;
use crate::intern::{Interner, Symbol};
use crate::span::{self, Range};

/// Which layer contributed an entity.
///
/// Ordering is the merge order: a higher layer overrides a lower one.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Layer {
    /// `<module>/doctype/<x>/<x>.json`.
    Schema = 1,
    /// `<app>/fixtures/*.json` — Custom Field and Property Setter records.
    Fixture = 2,
    /// The live site ([05](../../../docs/specs/05-live-bridge.md)). Never
    /// produced yet; the bridge lands with 014.
    Site = 3,
}

impl Layer {
    /// The layer's name, for hover text and the canonical fingerprint.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Schema => "schema",
            Self::Fixture => "fixture",
            Self::Site => "site",
        }
    }
}

/// Where an entity came from.
///
/// [04](../../../docs/specs/04-metadata-index.md) names this `Layer`, but a
/// layer alone cannot answer the hover question it exists for — "where did this
/// field come from" wants a file. The path is shared across every field of one
/// file rather than cloned 100k times.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Provenance {
    /// The layer.
    pub layer: Layer,
    /// The app that contributed it.
    pub app: Arc<str>,
    /// The file it was read from. `None` only for [`Layer::Site`], which has no
    /// file, so dropping the bridge drops exactly the entities with no path.
    pub source: Option<Arc<Path>>,
}

/// Byte ranges of the field properties later stages point an editor at.
///
/// Only the properties [21](../../../docs/specs/21-navigation.md) and
/// [23](../../../docs/specs/23-diagnostics.md) name: keeping a range per
/// property would cost more than the whole field table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FieldRanges {
    /// The `fieldname` value.
    pub fieldname: Option<Range>,
    /// The `fieldtype` value.
    pub fieldtype: Option<Range>,
    /// The `options` value.
    pub options: Option<Range>,
    /// The `fetch_from` value.
    pub fetch_from: Option<Range>,
    /// The `depends_on` value.
    pub depends_on: Option<Range>,
}

/// One field of a DocType.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Field {
    /// Interned `fieldname`.
    pub fieldname: Symbol,
    /// Interned `fieldtype`.
    pub fieldtype: Symbol,
    /// `label`.
    pub label: Option<String>,
    /// `options`, whose meaning depends on the fieldtype — see [`options_of`].
    pub options: Option<String>,
    /// `default`.
    pub default: Option<String>,
    /// `reqd`.
    pub reqd: bool,
    /// `hidden`.
    pub hidden: bool,
    /// `read_only`.
    pub read_only: bool,
    /// `unique`.
    pub unique: bool,
    /// `is_virtual`.
    pub is_virtual: bool,
    /// `permlevel`.
    pub permlevel: i64,
    /// `depends_on`.
    pub depends_on: Option<String>,
    /// `mandatory_depends_on`.
    pub mandatory_depends_on: Option<String>,
    /// `read_only_depends_on`.
    pub read_only_depends_on: Option<String>,
    /// `collapsible_depends_on`.
    pub collapsible_depends_on: Option<String>,
    /// `fetch_from`, `<link_fieldname>.<target_fieldname>` — see
    /// [`split_fetch_from`].
    pub fetch_from: Option<String>,
    /// `link_filters`, an opaque JSON blob.
    pub link_filters: Option<String>,
    /// The whole field object inside its file.
    pub range: Range,
    /// Ranges of the properties navigation and diagnostics point at.
    pub ranges: FieldRanges,
    /// Which layer and file it came from.
    pub provenance: Provenance,
}

/// A row of `permissions`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Permission {
    /// `role`.
    pub role: String,
    /// `permlevel`.
    pub permlevel: i64,
    /// Every set permission flag (`read`, `write`, `submit`, …), sorted.
    pub flags: Vec<String>,
}

/// A row of `links` — a dashboard connection to another DocType.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DocTypeLink {
    /// `link_doctype`.
    pub link_doctype: String,
    /// `link_fieldname` on that DocType.
    pub link_fieldname: Option<String>,
    /// `group`.
    pub group: Option<String>,
}

/// A row of `states` — a document state in the workflow-free state list.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct State {
    /// `title`.
    pub title: String,
    /// `color`.
    pub color: Option<String>,
}

/// Ranges of the document-level properties that name a field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MetaRanges {
    /// The `name` value.
    pub name: Option<Range>,
    /// The `title_field` value.
    pub title_field: Option<Range>,
    /// The `sort_field` value.
    pub sort_field: Option<Range>,
    /// The `image_field` value.
    pub image_field: Option<Range>,
    /// The `search_fields` value.
    pub search_fields: Option<Range>,
}

/// A DocType, after the layer merge.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DocType {
    /// The JSON `name`. Primary key, exact and case-sensitive.
    pub name: String,
    /// The JSON `module`, never the path.
    pub module: Option<String>,
    /// The app that won the definition.
    pub app: Arc<str>,
    /// The JSON file.
    pub path: Arc<Path>,
    /// `istable`.
    pub is_child: bool,
    /// `issingle`.
    pub is_single: bool,
    /// `is_submittable`.
    pub is_submittable: bool,
    /// `is_virtual`.
    pub is_virtual: bool,
    /// `is_tree`.
    pub is_tree: bool,
    /// `autoname`.
    pub autoname: Option<String>,
    /// `naming_rule`.
    pub naming_rule: Option<String>,
    /// `title_field`.
    pub title_field: Option<String>,
    /// `sort_field`.
    pub sort_field: Option<String>,
    /// `image_field`.
    pub image_field: Option<String>,
    /// `search_fields`, comma-delimited as Frappe stores it.
    pub search_fields: Option<String>,
    /// Declared fields, in `fields` order, Layer 2 additions appended by
    /// `insert_after`.
    pub fields: Vec<Field>,
    /// `field_order`, interned. May disagree with `fields` — that disagreement
    /// is a diagnostic, not something the index repairs.
    pub field_order: Vec<Symbol>,
    /// `permissions`.
    pub permissions: Vec<Permission>,
    /// `links`.
    pub links: Vec<DocTypeLink>,
    /// `states`.
    pub states: Vec<State>,
    /// The controller beside the JSON, once resolved.
    pub controller: Option<crate::controller::ControllerRef>,
    /// The whole document range.
    pub range: Range,
    /// Ranges of the document-level properties naming a field.
    pub meta_ranges: MetaRanges,
    /// Which layer and file the DocType itself came from.
    pub provenance: Provenance,
}

impl DocType {
    /// The field with this symbol, if declared. Implicit fields are not here —
    /// see [`DocType::has_field`].
    #[must_use]
    pub fn field(&self, fieldname: Symbol) -> Option<&Field> {
        self.fields
            .iter()
            .find(|field| field.fieldname == fieldname)
    }

    /// Every implicit field this DocType carries
    /// ([04](../../../docs/specs/04-metadata-index.md)).
    ///
    /// Implicit fields are deliberately not in [`DocType::fields`]: they have no
    /// range and no provenance, and a synthetic range would send
    /// go-to-definition to byte 0 of the JSON.
    pub fn implicit_fields(&self) -> impl Iterator<Item = &'static fieldtypes::Implicit> {
        fieldtypes::implicit(self.is_child)
    }

    /// Whether `fieldname` exists, declared or implicit. This is the question
    /// every "unknown field" check must ask.
    #[must_use]
    pub fn has_field(&self, interner: &Interner, fieldname: &str) -> bool {
        if fieldtypes::is_implicit(fieldname, self.is_child) {
            return true;
        }
        interner
            .get(fieldname)
            .is_some_and(|symbol| self.field(symbol).is_some())
    }

    /// Whether any declared field came from Layer 2.
    #[must_use]
    pub fn has_customizations(&self) -> bool {
        self.fields
            .iter()
            .any(|field| field.provenance.layer != Layer::Schema)
    }
}

/// What `options` means for a fieldtype
/// ([04](../../../docs/specs/04-metadata-index.md)).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Options<'a> {
    /// `Link`: the target DocType name.
    LinkTarget(&'a str),
    /// `Table` / `Table MultiSelect`: the child DocType name.
    ChildDocType(&'a str),
    /// `Dynamic Link`: the sibling **fieldname** holding the target DocType.
    DynamicSource(&'a str),
    /// `Select`: the choices.
    Select {
        /// Non-empty choices, in order.
        choices: Vec<&'a str>,
        /// Whether a leading blank line makes the field optional.
        optional: bool,
    },
    /// `Data`: a format hint (`Email`, `Phone`, …).
    FormatHint(&'a str),
    /// `Currency` / `Float`: a fieldname or dotted path to a currency field.
    CurrencyField(&'a str),
    /// Every other fieldtype treats `options` as opaque.
    Opaque(&'a str),
    /// No `options`, or an empty one.
    Absent,
}

/// Resolve `options` against its fieldtype.
#[must_use]
pub fn options_of<'a>(fieldtype: &str, options: Option<&'a str>) -> Options<'a> {
    let Some(options) = options.filter(|value| !value.is_empty()) else {
        return Options::Absent;
    };

    match fieldtype {
        "Link" => Options::LinkTarget(options.trim()),
        "Table" | "Table MultiSelect" => Options::ChildDocType(options.trim()),
        "Dynamic Link" => Options::DynamicSource(options.trim()),
        "Select" => {
            // A leading blank line is how Frappe spells "no selection allowed",
            // so it is meaning, not whitespace to trim away.
            let optional = options.starts_with('\n');
            Options::Select {
                choices: options.split('\n').filter(|c| !c.is_empty()).collect(),
                optional,
            }
        }
        "Data" => Options::FormatHint(options.trim()),
        "Currency" | "Float" => Options::CurrencyField(options.trim()),
        _ => Options::Opaque(options),
    }
}

/// The two halves of a `fetch_from`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FetchFrom<'a> {
    /// The `Link` field on this DocType.
    pub link_fieldname: &'a str,
    /// The field on the DocType that link points at.
    pub target_fieldname: &'a str,
}

/// Split `<link_fieldname>.<target_fieldname>`.
///
/// Exactly one dot, both halves non-empty. Anything else yields `None` rather
/// than a guess: [23](../../../docs/specs/23-diagnostics.md) checks both halves,
/// and a half invented from a malformed value is a false positive waiting to
/// happen.
#[must_use]
pub fn split_fetch_from(value: &str) -> Option<FetchFrom<'_>> {
    let value = value.trim();
    let (link, target) = value.split_once('.')?;
    if link.is_empty() || target.is_empty() || target.contains('.') {
        return None;
    }
    Some(FetchFrom {
        link_fieldname: link,
        target_fieldname: target,
    })
}

/// A DocType JSON as parsed, before interning and before the layer merge.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Raw {
    pub name: String,
    pub module: Option<String>,
    pub is_child: bool,
    pub is_single: bool,
    pub is_submittable: bool,
    pub is_virtual: bool,
    pub is_tree: bool,
    pub autoname: Option<String>,
    pub naming_rule: Option<String>,
    pub title_field: Option<String>,
    pub sort_field: Option<String>,
    pub image_field: Option<String>,
    pub search_fields: Option<String>,
    pub fields: Vec<RawField>,
    pub field_order: Vec<String>,
    pub permissions: Vec<Permission>,
    pub links: Vec<DocTypeLink>,
    pub states: Vec<State>,
    pub range: Range,
    pub meta_ranges: MetaRanges,
}

/// A field as parsed, before interning.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct RawField {
    pub fieldname: String,
    pub fieldtype: String,
    pub label: Option<String>,
    pub options: Option<String>,
    pub default: Option<String>,
    pub reqd: bool,
    pub hidden: bool,
    pub read_only: bool,
    pub unique: bool,
    pub is_virtual: bool,
    pub permlevel: i64,
    pub depends_on: Option<String>,
    pub mandatory_depends_on: Option<String>,
    pub read_only_depends_on: Option<String>,
    pub collapsible_depends_on: Option<String>,
    pub fetch_from: Option<String>,
    pub link_filters: Option<String>,
    pub range: Range,
    pub ranges: FieldRanges,
}

impl RawField {
    /// Intern the two repeated names and keep everything else as it is.
    pub(crate) fn into_field(self, interner: &mut Interner, provenance: Provenance) -> Field {
        Field {
            fieldname: interner.intern(&self.fieldname),
            fieldtype: interner.intern(&self.fieldtype),
            label: self.label,
            options: self.options,
            default: self.default,
            reqd: self.reqd,
            hidden: self.hidden,
            read_only: self.read_only,
            unique: self.unique,
            is_virtual: self.is_virtual,
            permlevel: self.permlevel,
            depends_on: self.depends_on,
            mandatory_depends_on: self.mandatory_depends_on,
            read_only_depends_on: self.read_only_depends_on,
            collapsible_depends_on: self.collapsible_depends_on,
            fetch_from: self.fetch_from,
            link_filters: self.link_filters,
            range: self.range,
            ranges: self.ranges,
            provenance,
        }
    }
}

/// Parse one DocType JSON.
///
/// `source` is the file text; the caller reads it so an open buffer can be
/// parsed without touching the disk.
pub(crate) fn parse(source: &str) -> Result<Raw, String> {
    let value: Value = serde_json::from_str(source).map_err(|error| error.to_string())?;
    let object = value.as_object().ok_or("not a JSON object")?;

    // Positions come from a second pass because serde_json discards them. A
    // document that parses but will not scan is a bug in the scanner, not in
    // the file, so it degrades to no ranges rather than dropping the DocType.
    let spans = span::scan(source).ok();
    let document = spans.as_ref();

    let name = string(object, "name").ok_or("no name")?;

    let field_spans = document
        .and_then(|node| node.value("fields"))
        .map(crate::span::Node::elements)
        .unwrap_or_default();

    let fields = object
        .get("fields")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .enumerate()
                .filter_map(|(at, row)| parse_field(row, field_spans.get(at)))
                .collect()
        })
        .unwrap_or_default();

    Ok(Raw {
        name,
        module: string(object, "module"),
        is_child: flag(object, "istable"),
        is_single: flag(object, "issingle"),
        is_submittable: flag(object, "is_submittable"),
        is_virtual: flag(object, "is_virtual"),
        is_tree: flag(object, "is_tree"),
        autoname: string(object, "autoname"),
        naming_rule: string(object, "naming_rule"),
        title_field: string(object, "title_field"),
        sort_field: string(object, "sort_field"),
        image_field: string(object, "image_field"),
        search_fields: string(object, "search_fields"),
        fields,
        field_order: object
            .get("field_order")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        permissions: parse_permissions(object),
        links: parse_links(object),
        states: parse_states(object),
        range: document.map_or_else(Range::none, |node| node.range),
        meta_ranges: MetaRanges {
            name: interior(document, "name"),
            title_field: interior(document, "title_field"),
            sort_field: interior(document, "sort_field"),
            image_field: interior(document, "image_field"),
            search_fields: interior(document, "search_fields"),
        },
    })
}

/// Parse a field object. A row without a `fieldname` is dropped: it can be
/// neither completed nor navigated to, and Frappe itself ignores it.
pub(crate) fn parse_field(value: &Value, node: Option<&span::Node>) -> Option<RawField> {
    let object = value.as_object()?;
    let fieldname = string(object, "fieldname")?;

    Some(RawField {
        fieldname,
        fieldtype: string(object, "fieldtype").unwrap_or_default(),
        label: string(object, "label"),
        options: string(object, "options"),
        default: string(object, "default"),
        reqd: flag(object, "reqd"),
        hidden: flag(object, "hidden"),
        read_only: flag(object, "read_only"),
        unique: flag(object, "unique"),
        is_virtual: flag(object, "is_virtual"),
        permlevel: object
            .get("permlevel")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        depends_on: string(object, "depends_on"),
        mandatory_depends_on: string(object, "mandatory_depends_on"),
        read_only_depends_on: string(object, "read_only_depends_on"),
        collapsible_depends_on: string(object, "collapsible_depends_on"),
        fetch_from: string(object, "fetch_from"),
        link_filters: string(object, "link_filters"),
        range: node.map_or_else(Range::none, |node| node.range),
        ranges: FieldRanges {
            fieldname: interior(node, "fieldname"),
            fieldtype: interior(node, "fieldtype"),
            options: interior(node, "options"),
            fetch_from: interior(node, "fetch_from"),
            depends_on: interior(node, "depends_on"),
        },
    })
}

fn parse_permissions(object: &serde_json::Map<String, Value>) -> Vec<Permission> {
    /// Every boolean column of DocPerm. Order is the order they are reported in.
    const FLAGS: [&str; 14] = [
        "select", "read", "write", "create", "delete", "submit", "cancel", "amend", "report",
        "export", "import", "share", "print", "email",
    ];

    object
        .get("permissions")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let row = row.as_object()?;
                    Some(Permission {
                        role: string(row, "role")?,
                        permlevel: row
                            .get("permlevel")
                            .and_then(Value::as_i64)
                            .unwrap_or_default(),
                        flags: FLAGS
                            .iter()
                            .filter(|flag| self::flag(row, flag))
                            .map(|flag| (*flag).to_string())
                            .collect(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_links(object: &serde_json::Map<String, Value>) -> Vec<DocTypeLink> {
    object
        .get("links")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let row = row.as_object()?;
                    Some(DocTypeLink {
                        link_doctype: string(row, "link_doctype")?,
                        link_fieldname: string(row, "link_fieldname"),
                        group: string(row, "group"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_states(object: &serde_json::Map<String, Value>) -> Vec<State> {
    object
        .get("states")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let row = row.as_object()?;
                    Some(State {
                        title: string(row, "title")?,
                        color: string(row, "color"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A non-empty string property.
pub(crate) fn string(object: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    let text = object.get(key)?.as_str()?;
    (!text.is_empty()).then(|| text.to_string())
}

/// A Frappe `Check`: `1` in JSON, but `true` and `"1"` both occur in fixtures.
pub(crate) fn flag(object: &serde_json::Map<String, Value>, key: &str) -> bool {
    match object.get(key) {
        Some(Value::Number(number)) => number.as_i64().is_some_and(|value| value != 0),
        Some(Value::Bool(value)) => *value,
        Some(Value::String(text)) => text == "1" || text == "true",
        _ => false,
    }
}

/// The interior range of a string property — the part without its quotes.
fn interior(node: Option<&span::Node>, key: &str) -> Option<Range> {
    Some(node?.value_range(key)?.interior())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_resolve_by_fieldtype() {
        assert_eq!(
            options_of("Link", Some("Library Member")),
            Options::LinkTarget("Library Member")
        );
        assert_eq!(
            options_of("Table MultiSelect", Some("Library Loan Item")),
            Options::ChildDocType("Library Loan Item")
        );
        assert_eq!(
            options_of("Dynamic Link", Some("reference_type")),
            Options::DynamicSource("reference_type")
        );
        assert_eq!(
            options_of("Data", Some("Email")),
            Options::FormatHint("Email")
        );
        assert_eq!(
            options_of("Currency", Some("currency")),
            Options::CurrencyField("currency")
        );
        assert_eq!(
            options_of("Attach", Some("whatever")),
            Options::Opaque("whatever")
        );
    }

    #[test]
    fn a_leading_blank_makes_a_select_optional() {
        assert_eq!(
            options_of("Select", Some("\nIssued\nReturned")),
            Options::Select {
                choices: vec!["Issued", "Returned"],
                optional: true,
            }
        );
        assert_eq!(
            options_of("Select", Some("Open\nClosed")),
            Options::Select {
                choices: vec!["Open", "Closed"],
                optional: false,
            }
        );
    }

    #[test]
    fn absent_and_empty_options_are_the_same_thing() {
        assert_eq!(options_of("Link", None), Options::Absent);
        assert_eq!(options_of("Link", Some("")), Options::Absent);
    }

    #[test]
    fn fetch_from_splits_only_when_it_has_exactly_one_dot() {
        assert_eq!(
            split_fetch_from("referred_by.email_id"),
            Some(FetchFrom {
                link_fieldname: "referred_by",
                target_fieldname: "email_id",
            })
        );
        assert_eq!(split_fetch_from("email_id"), None);
        assert_eq!(split_fetch_from("a.b.c"), None);
        assert_eq!(split_fetch_from(".email_id"), None);
        assert_eq!(split_fetch_from("referred_by."), None);
        assert_eq!(split_fetch_from(""), None);
    }

    #[test]
    fn a_check_reads_as_true_in_every_shape_frappe_writes_it() {
        let object = serde_json::json!({
            "number": 1, "zero": 0, "boolean": true, "text": "1", "other": "x"
        });
        let object = object.as_object().expect("object");

        assert!(flag(object, "number"));
        assert!(!flag(object, "zero"));
        assert!(flag(object, "boolean"));
        assert!(flag(object, "text"));
        assert!(!flag(object, "other"));
        assert!(!flag(object, "absent"));
    }

    #[test]
    fn parsing_keeps_the_range_of_every_field() {
        let source = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .expect("repo root")
                .join(
                    "tests/fixtures/bench/apps/library/library/library/doctype/library_member/library_member.json",
                ),
        )
        .expect("readable fixture");

        let raw = parse(&source).expect("parses");
        assert_eq!(raw.name, "Library Member");
        assert_eq!(raw.fields.len(), 8);
        assert_eq!(raw.field_order.len(), 8);
        assert_eq!(raw.autoname.as_deref(), Some("field:member_name"));
        assert_eq!(raw.title_field.as_deref(), Some("member_name"));
        assert!(!raw.is_child);

        let email = raw
            .fields
            .iter()
            .find(|field| field.fieldname == "email_id")
            .expect("email_id");
        let range = email.range;
        let text = range.text(&source).expect("field text");
        assert!(text.starts_with('{') && text.ends_with('}'), "{text}");
        assert!(text.contains("\"fieldname\": \"email_id\""));

        let options = email.ranges.options.expect("options range");
        assert_eq!(options.text(&source), Some("Email"));
    }

    #[test]
    fn a_malformed_document_reports_rather_than_panics() {
        assert!(parse("{").is_err());
        assert!(parse("[]").is_err());
        assert!(parse("{\"doctype\": \"DocType\"}").is_err(), "no name");
    }

    #[test]
    fn a_field_without_a_fieldname_is_dropped() {
        let raw = parse(r#"{"name": "X", "fields": [{"fieldtype": "Data"}, {"fieldname": "a"}]}"#)
            .expect("parses");
        assert_eq!(raw.fields.len(), 1);
        assert_eq!(raw.fields[0].fieldname, "a");
    }
}
