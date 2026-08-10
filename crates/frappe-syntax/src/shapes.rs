//! The call-shape tables of [10](../../../docs/specs/10-surface-python.md) and
//! [11](../../../docs/specs/11-surface-javascript.md), as data.
//!
//! Only positions the specs spell out are listed. Where a spec writes `(D, …)`
//! the trailing arguments are unspecified, so nothing beyond argument 0 is
//! read: guessing at an unwritten position is how a surface starts reporting on
//! valid code.

use crate::matches::{PathKind, PatternId};

/// What one positional argument means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arg {
    /// A name, a value, a callback — read nothing.
    Ignore,
    /// A DocType name.
    DocType,
    /// A single fieldname.
    Field,
    /// `"F asc, G desc"` — comma separated, direction optional.
    OrderBy,
    /// `F` or `[F, …]`.
    Fields,
    /// `F`, `[F, …]`, or `{F: value}` — `db.set_value`'s third argument.
    FieldsOrMap,
    /// A filters expression in any of the three shapes of
    /// [10](../../../docs/specs/10-surface-python.md).
    Filters,
    /// An options object whose keys are the query kwargs.
    Options,
    /// A dotted path, with what the caller expects it to name.
    Path(PathKind),
    /// A dict whose keys are fields of the child DocType.
    ChildFields,
    /// A `hooks.py` key.
    HookKey,
    /// A DocField property name.
    Property,
    /// A method on the DocType this call anchors to.
    Method,
}

/// A recognized free-function call.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Shape {
    /// Stable identity, and the dotted callee it matches.
    pub id: PatternId,
    /// Positional arguments, in order.
    pub args: &'static [Arg],
    /// Whether the fieldname kwargs of
    /// [10](../../../docs/specs/10-surface-python.md) apply.
    pub kwargs: bool,
    /// A recognized constructor: binds its assignment target
    /// ([06](../../../docs/specs/06-analysis-pipeline.md)).
    pub constructor: bool,
}

impl Shape {
    const fn new(callee: &'static str, args: &'static [Arg]) -> Self {
        Self {
            id: PatternId(callee),
            args,
            kwargs: false,
            constructor: false,
        }
    }

    const fn with_kwargs(mut self) -> Self {
        self.kwargs = true;
        self
    }

    const fn constructor(mut self) -> Self {
        self.constructor = true;
        self
    }

    /// The dotted callee this shape matches.
    pub(crate) fn callee(&self) -> &'static str {
        self.id.0
    }
}

/// A method call on an anchored receiver — `doc.get(F)`, `frm.set_value(F)`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Method {
    /// Stable identity, written `<receiver>.<method>`.
    pub id: PatternId,
    /// The method name.
    pub name: &'static str,
    /// Positional arguments, in order.
    pub args: &'static [Arg],
}

impl Method {
    const fn new(id: &'static str, name: &'static str, args: &'static [Arg]) -> Self {
        Self {
            id: PatternId(id),
            name,
            args,
        }
    }
}

use Arg::{ChildFields, DocType, Field, Fields, FieldsOrMap, Filters, HookKey, Ignore, Options};

/// `frappe.*` calls in Python — [10](../../../docs/specs/10-surface-python.md).
pub(crate) const PYTHON: &[Shape] = &[
    Shape::new("frappe.get_doc", &[DocType, Ignore]).constructor(),
    Shape::new("frappe.get_cached_doc", &[DocType, Ignore]).constructor(),
    Shape::new("frappe.new_doc", &[DocType]).constructor(),
    Shape::new("frappe.get_last_doc", &[DocType]).constructor(),
    Shape::new("frappe.get_single", &[DocType]).constructor(),
    Shape::new("frappe.get_all", &[DocType]).with_kwargs(),
    Shape::new("frappe.get_list", &[DocType]).with_kwargs(),
    Shape::new("frappe.db.get_all", &[DocType]).with_kwargs(),
    Shape::new("frappe.db.get_list", &[DocType]).with_kwargs(),
    Shape::new("frappe.db.get_value", &[DocType, Filters, Fields]).with_kwargs(),
    Shape::new("frappe.db.get_values", &[DocType, Filters, Fields]).with_kwargs(),
    Shape::new(
        "frappe.db.set_value",
        &[DocType, Ignore, FieldsOrMap, Ignore],
    ),
    Shape::new("frappe.db.exists", &[DocType]).with_kwargs(),
    Shape::new("frappe.db.count", &[DocType]).with_kwargs(),
    Shape::new("frappe.db.delete", &[DocType]).with_kwargs(),
    Shape::new("frappe.db.get_single_value", &[DocType, Field]),
    Shape::new("frappe.db.set_single_value", &[DocType, Field, Ignore]),
    Shape::new("frappe.get_value", &[DocType]).with_kwargs(),
    Shape::new("frappe.get_cached_value", &[DocType, Ignore, Fields]),
    Shape::new("frappe.get_meta", &[DocType]),
    Shape::new("frappe.get_hooks", &[HookKey]),
    Shape::new("frappe.delete_doc", &[DocType, Ignore]),
    Shape::new("frappe.rename_doc", &[DocType, Ignore, Ignore]),
    Shape::new("frappe.has_permission", &[DocType]),
    Shape::new("frappe.qb.DocType", &[DocType]),
    Shape::new("frappe.call", &[Arg::Path(PathKind::Whitelisted)]),
    Shape::new("frappe.get_attr", &[Arg::Path(PathKind::Callable)]),
    Shape::new("frappe.enqueue", &[Arg::Path(PathKind::Callable)]),
    Shape::new("frappe.enqueue_doc", &[DocType, Ignore, Arg::Method]),
];

/// Methods on `self` or a name bound per
/// [06](../../../docs/specs/06-analysis-pipeline.md).
pub(crate) const PYTHON_METHODS: &[Method] = &[
    Method::new("doc.get", "get", &[Field]),
    Method::new("doc.set", "set", &[Field, Ignore]),
    Method::new("doc.get_value", "get_value", &[Field]),
    Method::new("doc.append", "append", &[Field, ChildFields]),
    Method::new("doc.db_set", "db_set", &[FieldsOrMap, Ignore]),
    Method::new("doc.get_formatted", "get_formatted", &[Field]),
];

/// `frappe.*` calls in client scripts —
/// [11](../../../docs/specs/11-surface-javascript.md).
pub(crate) const JAVASCRIPT: &[Shape] = &[
    Shape::new("frappe.model.get_value", &[DocType, Ignore, Field]),
    Shape::new("frappe.db.get_value", &[DocType, Filters, Fields]),
    Shape::new(
        "frappe.db.set_value",
        &[DocType, Ignore, FieldsOrMap, Ignore],
    ),
    Shape::new("frappe.db.get_list", &[DocType, Options]),
    Shape::new("frappe.db.get_doc", &[DocType, Ignore]),
    Shape::new("frappe.db.exists", &[DocType]),
    Shape::new("frappe.db.count", &[DocType]),
    Shape::new("frappe.db.delete_doc", &[DocType, Ignore]),
    Shape::new("frappe.new_doc", &[DocType]).constructor(),
    Shape::new("frappe.xcall", &[Arg::Path(PathKind::Whitelisted), Ignore]),
];

/// `frm.*` and `cur_frm.*` methods —
/// [11](../../../docs/specs/11-surface-javascript.md).
pub(crate) const JAVASCRIPT_METHODS: &[Method] = &[
    Method::new("frm.set_value", "set_value", &[FieldsOrMap, Ignore]),
    Method::new("frm.get_field", "get_field", &[Field]),
    Method::new("frm.refresh_field", "refresh_field", &[Field]),
    Method::new("frm.get_docfield", "get_docfield", &[Field]),
    Method::new(
        "frm.set_df_property",
        "set_df_property",
        &[Field, Arg::Property, Ignore],
    ),
    Method::new("frm.toggle_display", "toggle_display", &[Fields, Ignore]),
    Method::new("frm.toggle_enable", "toggle_enable", &[Fields, Ignore]),
    Method::new("frm.toggle_reqd", "toggle_reqd", &[Fields, Ignore]),
    Method::new("frm.add_child", "add_child", &[Field, ChildFields]),
];

/// Shapes that do not fit the tables above and are matched by their own code.
pub(crate) mod special {
    use crate::matches::PatternId;

    /// `frappe.get_doc({"doctype": D, F: …})`.
    pub(crate) const PY_GET_DOC_DICT: PatternId = PatternId("frappe.get_doc{}");
    /// The `@frappe.whitelist(...)` decorator —
    /// [07](../../../docs/specs/07-dotted-paths.md).
    pub(crate) const PY_WHITELIST: PatternId = PatternId("frappe.whitelist");
    /// Attribute access on a bound name: `doc.<attr>`.
    pub(crate) const PY_ATTRIBUTE: PatternId = PatternId("doc.attribute");
    /// `_( … )`.
    pub(crate) const PY_TRANSLATE: PatternId = PatternId("_");

    /// `frappe.ui.form.on(D, {…})` and the 3-argument legacy form.
    pub(crate) const JS_FORM_ON: PatternId = PatternId("frappe.ui.form.on");
    /// `frm.doc.<attr>`.
    pub(crate) const JS_FRM_DOC: PatternId = PatternId("frm.doc.attribute");
    /// `frm.set_query(F, fn)` / `frm.set_query(F, table_F, fn)`.
    pub(crate) const JS_SET_QUERY: PatternId = PatternId("frm.set_query");
    /// `frm.call({ method, args })`.
    pub(crate) const JS_FRM_CALL: PatternId = PatternId("frm.call");
    /// `frappe.call({ method, args })`.
    pub(crate) const JS_CALL: PatternId = PatternId("frappe.call");
    /// `frappe.set_route("Form", D, name)`.
    pub(crate) const JS_SET_ROUTE: PatternId = PatternId("frappe.set_route");
    /// `frappe.listview_settings[D] = {…}`.
    pub(crate) const JS_LISTVIEW_SETTINGS: PatternId = PatternId("frappe.listview_settings[]");
    /// `locals[D][name]`.
    pub(crate) const JS_LOCALS: PatternId = PatternId("locals[]");
    /// `__( … )`.
    pub(crate) const JS_TRANSLATE: PatternId = PatternId("__");
}

/// The kwargs of [10](../../../docs/specs/10-surface-python.md)'s fieldname
/// table, and what each one holds.
pub(crate) fn kwarg(name: &str) -> Option<Arg> {
    match name {
        "fields" | "pluck" | "group_by" => Some(Fields),
        "filters" | "or_filters" => Some(Filters),
        "order_by" => Some(Arg::OrderBy),
        _ => None,
    }
}

/// Constructors that bind a local per
/// [06](../../../docs/specs/06-analysis-pipeline.md), as bare names — the
/// `from frappe import get_doc` form of
/// [10](../../../docs/specs/10-surface-python.md)'s aliasing rule.
pub(crate) fn python_shape(callee: &str) -> Option<&'static Shape> {
    PYTHON.iter().find(|shape| shape.callee() == callee)
}

/// The JavaScript counterpart of [`python_shape`].
pub(crate) fn javascript_shape(callee: &str) -> Option<&'static Shape> {
    JAVASCRIPT.iter().find(|shape| shape.callee() == callee)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn no_language_lists_one_pattern_id_twice() {
        // The two languages share ids on purpose — `frappe.db.get_value` is
        // the same shape either side — and the buffer says which grammar it is.
        for table in [PYTHON, JAVASCRIPT] {
            let mut seen = HashSet::new();
            for shape in table {
                assert!(seen.insert(shape.id), "duplicate pattern id {}", shape.id);
            }
        }
        for table in [PYTHON_METHODS, JAVASCRIPT_METHODS] {
            let mut seen = HashSet::new();
            for method in table {
                assert!(seen.insert(method.id), "duplicate pattern id {}", method.id);
            }
        }
    }

    #[test]
    fn the_constructors_are_the_five_specs_06_names() {
        let mut names: Vec<&str> = PYTHON
            .iter()
            .filter(|shape| shape.constructor)
            .map(Shape::callee)
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "frappe.get_cached_doc",
                "frappe.get_doc",
                "frappe.get_last_doc",
                "frappe.get_single",
                "frappe.new_doc",
            ]
        );
    }

    #[test]
    fn a_callee_outside_the_tables_matches_nothing() {
        // specs/10 excludes raw SQL, and `frappe.msgprint` names no position.
        assert!(python_shape("frappe.db.sql").is_none());
        assert!(python_shape("frappe.msgprint").is_none());
        assert!(javascript_shape("frappe.db.sql").is_none());
    }

    #[test]
    fn kwargs_outside_the_fieldname_table_are_not_fieldnames() {
        assert_eq!(kwarg("fields"), Some(Arg::Fields));
        assert_eq!(kwarg("order_by"), Some(Arg::OrderBy));
        assert_eq!(kwarg("or_filters"), Some(Arg::Filters));
        assert_eq!(kwarg("limit_page_length"), None);
        assert_eq!(kwarg("as_dict"), None);
        assert_eq!(kwarg("doctype"), None);
    }
}
