//! Every row of spec 10, extracted and resolved against the fixture bench.
//!
//! Each pattern is checked twice: once with a literal that must resolve, once
//! with the dynamic form that must not. The negative half is the point — spec
//! 23's precision rule lives or dies on positions that stay silent, and a test
//! suite of positives alone would pass on a resolver that guesses.

use frappe_index::index::Index;
use frappe_index::resolve::{Target, resolve_all};
use frappe_syntax::{Context, Language, Role, extract};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn bench() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("tests/fixtures/bench")
}

fn index() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| Index::build(&bench()))
}

/// Extract and resolve one snippet. No file, no buffer — spec 32's unit level.
fn resolve(source: &str) -> Vec<frappe_index::Resolved> {
    let mut parser = Language::Python.parser().expect("the grammar loads");
    let tree = parser.parse(source, None).expect("a tree");
    let matches = extract(Language::Python, &tree, source, Context::default());
    resolve_all(index(), &matches)
}

/// The targets of every position of one role, in source order.
fn targets(source: &str, role: Role) -> Vec<Target> {
    resolve(source)
        .into_iter()
        .filter(|resolved| resolved.role == role)
        .map(|resolved| resolved.target)
        .collect()
}

fn doctypes(source: &str) -> Vec<Target> {
    targets(source, Role::DocType)
}

fn fields(source: &str) -> Vec<Target> {
    targets(source, Role::Field)
}

fn is_doctype(target: &Target, expected: &str) -> bool {
    matches!(target, Target::DocType { name } if name == expected)
}

fn is_field(target: &Target, expected_doctype: &str, expected_field: &str) -> bool {
    matches!(
        target,
        Target::Field { doctype, fieldname, .. }
            if doctype == expected_doctype && fieldname == expected_field
    )
}

// --- DocType positions: the `frappe.*` and `frappe.db.*` tables --------------

#[test]
fn every_doctype_position_in_spec_10_resolves() {
    // One row of the table per line, each naming a DocType the fixture has.
    let calls = [
        r#"frappe.get_doc("Library Member", name)"#,
        r#"frappe.get_doc({"doctype": "Library Member"})"#,
        r#"frappe.get_cached_doc("Library Member", name)"#,
        r#"frappe.new_doc("Library Member")"#,
        r#"frappe.get_last_doc("Library Member")"#,
        r#"frappe.get_all("Library Member")"#,
        r#"frappe.get_list("Library Member")"#,
        r#"frappe.db.get_all("Library Member")"#,
        r#"frappe.db.get_list("Library Member")"#,
        r#"frappe.db.get_value("Library Member", name, "member_name")"#,
        r#"frappe.db.get_values("Library Member", name, "member_name")"#,
        r#"frappe.db.set_value("Library Member", name, "member_name", v)"#,
        r#"frappe.db.exists("Library Member", name)"#,
        r#"frappe.db.count("Library Member")"#,
        r#"frappe.db.delete("Library Member")"#,
        r#"frappe.db.get_single_value("Library Settings", "loan_period")"#,
        r#"frappe.db.set_single_value("Library Settings", "loan_period", 7)"#,
        r#"frappe.get_value("Library Member", name)"#,
        r#"frappe.get_cached_value("Library Member", name, "member_name")"#,
        r#"frappe.get_meta("Library Member")"#,
        r#"frappe.get_single("Library Settings")"#,
        r#"frappe.delete_doc("Library Member", name)"#,
        r#"frappe.rename_doc("Library Member", old, new)"#,
        r#"frappe.has_permission("Library Member")"#,
        r#"frappe.qb.DocType("Library Member")"#,
    ];

    for call in calls {
        let found = doctypes(&format!("import frappe\n{call}\n"));
        assert!(
            found
                .iter()
                .any(|target| is_doctype(target, "Library Member")
                    || is_doctype(target, "Library Settings")),
            "{call} produced {found:?}"
        );
    }
}

#[test]
fn the_same_positions_stay_silent_on_a_dynamic_argument() {
    // The negative half of the table above. A variable in a DocType position
    // must never reach the index — specs/06.
    let calls = [
        "frappe.get_doc(doctype, name)",
        "frappe.get_cached_doc(doctype, name)",
        "frappe.new_doc(doctype)",
        "frappe.get_all(doctype)",
        "frappe.db.get_value(doctype, name, fieldname)",
        "frappe.db.set_value(doctype, name, fieldname, v)",
        "frappe.db.exists(doctype, name)",
        "frappe.db.get_single_value(doctype, fieldname)",
        "frappe.get_meta(doctype)",
        "frappe.delete_doc(doctype, name)",
        "frappe.qb.DocType(doctype)",
    ];

    for call in calls {
        let found = resolve(&format!("import frappe\n{call}\n"));
        assert!(
            found
                .iter()
                .all(|resolved| resolved.target == Target::Unresolvable),
            "{call} produced {found:?}"
        );
    }
}

#[test]
fn a_misspelled_doctype_is_named_with_its_near_miss() {
    let found = doctypes("import frappe\nfrappe.get_doc(\"library member\", name)\n");

    let Some(Target::UnknownDocType { name, similar }) = found.first() else {
        panic!("expected an unknown DocType, got {found:?}");
    };
    assert_eq!(name, "library member");
    // Frappe is case-sensitive here, so this is a hint and not a resolution.
    assert_eq!(similar, &["Library Member"]);
}

// --- Fieldname positions -----------------------------------------------------

#[test]
fn every_fieldname_kwarg_resolves_against_the_call_anchor() {
    let sources = [
        r#"frappe.get_all("Library Member", fields=["member_name"])"#,
        r#"frappe.get_all("Library Member", fields="member_name")"#,
        r#"frappe.get_all("Library Member", filters={"member_name": "x"})"#,
        r#"frappe.get_all("Library Member", filters=[["member_name", "=", "x"]])"#,
        r#"frappe.get_all("Library Member", or_filters={"member_name": "x"})"#,
        r#"frappe.get_all("Library Member", pluck="member_name")"#,
        r#"frappe.get_all("Library Member", order_by="member_name asc")"#,
        r#"frappe.get_all("Library Member", group_by="member_name")"#,
        r#"frappe.db.set_value("Library Member", name, {"member_name": "x"})"#,
    ];

    for source in sources {
        let found = fields(&format!("import frappe\n{source}\n"));
        assert!(
            found
                .iter()
                .any(|target| is_field(target, "Library Member", "member_name")),
            "{source} produced {found:?}"
        );
    }
}

#[test]
fn a_four_tuple_filter_reanchors_to_its_own_doctype() {
    let found = fields(
        "import frappe\n\
         frappe.get_all(\"Library Member\", filters=[[\"Library Loan Item\", \"book_title\", \"=\", \"x\"]])\n",
    );

    // `book_title` is a field of Library Loan Item and not of Library Member;
    // resolving it against the call's own anchor would report it as unknown.
    assert!(
        found
            .iter()
            .any(|target| is_field(target, "Library Loan Item", "book_title")),
        "{found:?}"
    );
}

#[test]
fn an_unknown_field_is_reported_against_the_doctype_that_lacks_it() {
    let found = fields("import frappe\nfrappe.get_all(\"Library Member\", pluck=\"emial_id\")\n");

    assert!(
        matches!(
            found.first(),
            Some(Target::UnknownField { doctype, fieldname })
                if doctype == "Library Member" && fieldname == "emial_id"
        ),
        "{found:?}"
    );
}

#[test]
fn implicit_fields_are_never_unknown() {
    // specs/04: these are absent from the JSON and MUST be completable.
    for fieldname in [
        "name",
        "owner",
        "creation",
        "modified",
        "docstatus",
        "idx",
        "doctype",
    ] {
        let source = format!(
            "import frappe\nfrappe.get_all(\"Library Member\", fields=[\"{fieldname}\"])\n"
        );
        let found = fields(&source);
        assert!(
            matches!(found.first(), Some(Target::Field { implicit: true, .. })),
            "{fieldname} produced {found:?}"
        );
    }

    // A child DocType additionally carries the parent trio.
    for fieldname in ["parent", "parentfield", "parenttype"] {
        let source = format!(
            "import frappe\nfrappe.get_all(\"Library Loan Item\", fields=[\"{fieldname}\"])\n"
        );
        assert!(
            matches!(
                fields(&source).first(),
                Some(Target::Field { implicit: true, .. })
            ),
            "{fieldname} is a child field"
        );
    }
}

#[test]
fn the_skip_rules_produce_no_position_at_all() {
    // specs/10: skipped means no position, not an unresolved one — a position
    // that exists is one a later check can decide to report on.
    let sources = [
        r#"frappe.get_all("Library Member", fields=["count(name) as total"])"#,
        r#"frappe.get_all("Library Member", fields=["referred_by.member_name"])"#,
        r#"frappe.get_all("Library Member", fields=["`tabLibrary Member`.member_name"])"#,
        r#"frappe.db.sql("select emial_id from `tabLibrary Member`")"#,
    ];

    for source in sources {
        let found = fields(&format!("import frappe\n{source}\n"));
        assert!(found.is_empty(), "{source} produced {found:?}");
    }
}

#[test]
fn a_field_on_an_unknown_doctype_reports_the_doctype_only() {
    // One mistake, one diagnostic: the field cannot be judged against a
    // DocType the index does not have.
    let found =
        resolve("import frappe\nfrappe.get_all(\"Libary Member\", pluck=\"member_name\")\n");

    let doctype = found.iter().filter(|r| r.role == Role::DocType).count();
    assert_eq!(doctype, 1);
    assert!(
        found
            .iter()
            .filter(|r| r.role == Role::Field)
            .all(|r| r.target == Target::Unresolvable),
        "{found:?}"
    );
}

// --- Document methods --------------------------------------------------------

#[test]
fn a_bound_local_carries_its_doctype_to_document_methods() {
    let found = fields(
        "import frappe\n\
         def f():\n\
         \tdoc = frappe.get_doc(\"Library Member\", name)\n\
         \tdoc.get(\"member_name\")\n\
         \tdoc.db_set(\"email_id\", x)\n",
    );

    assert!(
        found
            .iter()
            .any(|target| is_field(target, "Library Member", "member_name")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|target| is_field(target, "Library Member", "email_id")),
        "{found:?}"
    );
}

#[test]
fn a_reassigned_local_drops_its_binding() {
    // specs/06 bounds inference at "drop on any reassignment".
    let found = fields(
        "import frappe\n\
         def f(other):\n\
         \tdoc = frappe.get_doc(\"Library Member\", name)\n\
         \tdoc = other\n\
         \tdoc.get(\"member_name\")\n",
    );

    assert!(
        found.iter().all(|target| *target == Target::Unresolvable),
        "{found:?}"
    );
}

#[test]
fn append_binds_dict_keys_to_the_child_doctype() {
    let found = resolve(
        "import frappe\n\
         def f():\n\
         \tdoc = frappe.get_doc(\"Library Member\", name)\n\
         \tdoc.append(\"loans\", {\"book_title\": t, \"due_date\": d})\n",
    );

    // The `Table` field itself belongs to the parent.
    assert!(
        found
            .iter()
            .any(|r| r.role == Role::Field && is_field(&r.target, "Library Member", "loans")),
        "{found:?}"
    );
    // Its dict keys belong to the child behind it — the join only this side
    // can make.
    for fieldname in ["book_title", "due_date"] {
        assert!(
            found.iter().any(|r| r.role == Role::ChildField
                && is_field(&r.target, "Library Loan Item", fieldname)),
            "{fieldname} produced {found:?}"
        );
    }
}

#[test]
fn append_to_an_unknown_field_resolves_no_child_keys() {
    let found = resolve(
        "import frappe\n\
         def f():\n\
         \tdoc = frappe.get_doc(\"Library Member\", name)\n\
         \tdoc.append(\"no_such_table\", {\"book_title\": t})\n",
    );

    assert!(
        found
            .iter()
            .filter(|r| r.role == Role::ChildField)
            .all(|r| r.target == Target::Unresolvable),
        "{found:?}"
    );
}

#[test]
fn self_binds_to_the_controllers_doctype() {
    let source = "import frappe\n\
                  class LibraryMember(Document):\n\
                  \tdef validate(self):\n\
                  \t\tself.get(\"member_name\")\n";
    let mut parser = Language::Python.parser().expect("the grammar loads");
    let tree = parser.parse(source, None).expect("a tree");

    // The controller's DocType comes from the file's location, which the
    // server knows and the snippet does not.
    let matches = extract(
        Language::Python,
        &tree,
        source,
        Context {
            self_doctype: Some("Library Member"),
            self_class: None,
        },
    );
    let found = resolve_all(index(), &matches);

    assert!(
        found
            .iter()
            .any(|r| is_field(&r.target, "Library Member", "member_name")),
        "{found:?}"
    );
}

#[test]
fn a_table_row_loop_binds_to_the_child() {
    let found = fields(
        "import frappe\n\
         def f():\n\
         \tdoc = frappe.get_doc(\"Library Member\", name)\n\
         \tfor row in doc.loans:\n\
         \t\trow.get(\"book_title\")\n",
    );

    // Extraction stops at `(Library Member, loans)`; the child DocType behind
    // the Table field is the schema lookup this layer exists for.
    assert!(
        found
            .iter()
            .any(|target| is_field(target, "Library Loan Item", "book_title")),
        "{found:?}"
    );
}

// --- Paths and whitelisting --------------------------------------------------

#[test]
fn a_whitelisted_path_resolves_and_an_undecorated_one_does_not() {
    let found = resolve(
        "import frappe\n\
         frappe.call(\"library.api.get_member_summary\")\n",
    );
    let path = found
        .iter()
        .find(|r| matches!(r.role, Role::Path(_)))
        .expect("a path position");
    assert!(path.target.is_resolved(), "{path:?}");

    // `library.api.loan_period` exists but carries no decorator.
    let found = resolve("import frappe\nfrappe.call(\"library.api.loan_period\")\n");
    let path = found
        .iter()
        .find(|r| matches!(r.role, Role::Path(_)))
        .expect("a path position");
    let Target::Path(resolution) = &path.target else {
        panic!("expected a path target, got {path:?}");
    };
    assert_eq!(resolution.outcome, frappe_index::paths::Outcome::WrongKind);
}

#[test]
fn a_path_into_an_app_outside_the_workspace_stays_unresolved() {
    // specs/11: a path that resolves nowhere is a weaker signal than one that
    // resolves to something unwhitelisted — it may live in another bench.
    let found = resolve("import frappe\nfrappe.call(\"erpnext.accounts.utils.get_balance_on\")\n");
    let path = found
        .iter()
        .find(|r| matches!(r.role, Role::Path(_)))
        .expect("a path position");

    let Target::Path(resolution) = &path.target else {
        panic!("expected a path target");
    };
    assert_eq!(resolution.outcome, frappe_index::paths::Outcome::Unresolved);
}

#[test]
fn aliasing_is_recognized_per_file() {
    // specs/10: `from frappe import get_doc` is recognized; `import frappe as f`
    // deliberately is not, and must stay silent rather than guess.
    let recognized = doctypes("from frappe import get_doc\nget_doc(\"Library Member\", name)\n");
    assert!(
        recognized
            .iter()
            .any(|target| is_doctype(target, "Library Member")),
        "{recognized:?}"
    );

    let unrecognized = resolve("import frappe as f\nf.get_doc(\"Libary Member\", name)\n");
    assert!(
        unrecognized.is_empty(),
        "an unrecognized alias must produce nothing: {unrecognized:?}"
    );
}

// --- Attribute access is not the same question as a fieldname literal --------

#[test]
fn document_members_reached_by_attribute_access_are_not_unknown_fields() {
    // `flags` alone would fire on 24 DocTypes across frappe. specs/23's
    // governing rule makes that a bug in the check, not in frappe.
    let found = fields(
        "import frappe\n\
         def f():\n\
         \tdoc = frappe.get_doc(\"Library Member\", name)\n\
         \tdoc.flags.ignore_permissions = True\n\
         \tdoc.save()\n\
         \tdoc.meta\n",
    );

    assert!(
        found
            .iter()
            .all(|target| !matches!(target, Target::UnknownField { .. })),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|target| matches!(target, Target::DocumentMember { name } if name == "flags")),
        "{found:?}"
    );
}

#[test]
fn a_controller_method_reached_by_attribute_access_resolves_to_the_method() {
    // A bare attribute read, not a call: `doc.method(...)` is a call shape and
    // spec 10's `doc.<attr>` row is about the attribute itself.
    let found = fields(
        "import frappe\n\
         def f():\n\
         \tdoc = frappe.get_doc(\"Library Member\", name)\n\
         \thandler = doc.add_loan\n",
    );

    assert!(
        found.iter().any(|target| matches!(
            target,
            Target::Method { doctype, name } if doctype == "Library Member" && name == "add_loan"
        )),
        "{found:?}"
    );
}

#[test]
fn a_field_still_wins_a_tie_with_a_document_member() {
    // Library Membership has a real `amount` field; `Document` has `get`. A
    // DocType whose schema declares a name keeps it.
    let found = fields(
        "import frappe\n\
         def f():\n\
         \tdoc = frappe.get_doc(\"Library Membership\", name)\n\
         \tdoc.amount\n",
    );

    assert!(
        found
            .iter()
            .any(|target| is_field(target, "Library Membership", "amount")),
        "{found:?}"
    );
}

#[test]
fn a_genuinely_wrong_fieldname_is_still_reported() {
    // The exemptions above must not swallow the check they protect.
    let found = fields(
        "import frappe\n\
         def f():\n\
         \tdoc = frappe.get_doc(\"Library Member\", name)\n\
         \tdoc.emial_id\n",
    );

    assert!(
        found.iter().any(|target| matches!(
            target,
            Target::UnknownField { fieldname, .. } if fieldname == "emial_id"
        )),
        "{found:?}"
    );
}
