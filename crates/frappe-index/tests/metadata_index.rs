//! The metadata graph against the fixture bench — spec 04 and spec 07.
//!
//! Every number here is read out of `tests/fixtures/README.md`, which is the
//! description of the corpus; a change to either has to change the other.

use std::path::{Path, PathBuf};

use frappe_index::doctype::Layer;
use frappe_index::index::ConflictCode;
use frappe_index::paths::{Kind, Outcome, SymbolKind};
use frappe_index::{Confidence, Index, Options};

fn bench() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives two levels below the repo root")
        .join("tests/fixtures/bench")
}

fn index() -> Index {
    Index::build(&bench())
}

// --- Entities ----------------------------------------------------------------

#[test]
fn the_fixture_bench_indexes_to_its_readme() {
    let index = index();
    let status = index.status();

    // 9 DocType records, two of which share the name `Library Availability`.
    assert_eq!(status.doctypes, 8, "{:?}", status);
    assert_eq!(status.apps, 3);
    assert_eq!(status.fields, 41);
    assert_eq!(status.controllers, 5);
    assert_eq!(status.whitelisted_methods, 5);
    assert_eq!(status.conflicts, 2);
    assert_eq!(status.confidence, Confidence::Static);
    assert_eq!(status.frappe_version.as_deref(), Some("17.0.0-dev"));
}

#[test]
fn the_malformed_doctype_is_dropped_and_the_rest_of_its_app_survives() {
    let index = index();

    assert_eq!(index.doctype("Circulation Malformed"), None);
    assert_eq!(index.problems().len(), 1, "{:?}", index.problems());
    assert!(
        index.problems()[0]
            .path
            .ends_with("circulation_malformed.json")
    );
    assert!(index.problems()[0].message.starts_with("malformed JSON"));

    // Same app, still indexed.
    assert!(index.doctype("Loan").is_some());
    assert!(index.doctype("Circulation Broken Field").is_some());
}

#[test]
fn the_doctype_flags_come_from_the_json_keys_that_carry_them() {
    let index = index();

    let child = index.doctype("Library Loan Item").expect("child");
    assert!(child.is_child);
    assert!(!child.is_single);

    let single = index.doctype("Library Settings").expect("single");
    assert!(single.is_single);

    let submittable = index.doctype("Library Membership").expect("submittable");
    assert!(submittable.is_submittable);

    let virtual_doctype = index.doctype("Library Availability").expect("virtual");
    assert!(virtual_doctype.is_virtual);

    let plain = index.doctype("Library Member").expect("plain");
    assert!(!plain.is_child && !plain.is_single && !plain.is_submittable && !plain.is_virtual);
    assert_eq!(plain.autoname.as_deref(), Some("field:member_name"));
    assert_eq!(plain.title_field.as_deref(), Some("member_name"));
    assert_eq!(plain.module.as_deref(), Some("Library"));
}

#[test]
fn lookup_is_exact_and_case_insensitive_matches_are_only_a_suggestion() {
    let index = index();

    assert!(index.doctype("Library Member").is_some());
    assert_eq!(index.doctype("library member"), None);
    assert_eq!(index.doctype("LIBRARY MEMBER"), None);
    assert_eq!(index.doctype("Libary Member"), None);

    assert_eq!(
        index.case_insensitive_matches("library member"),
        ["Library Member"]
    );
    assert!(index.case_insensitive_matches("Library Member").is_empty());
}

#[test]
fn a_json_path_resolves_back_to_the_doctype_it_defines() {
    let index = index();
    let path =
        bench().join("apps/library/library/library/doctype/library_member/library_member.json");

    assert_eq!(
        index.doctype_at(&path).map(|doctype| doctype.name.as_str()),
        Some("Library Member")
    );

    // The losing half of a duplicate is not the file the merge kept.
    let loser = bench().join(
        "apps/library/library/library/doctype/library_availability/library_availability.json",
    );
    assert_eq!(index.doctype_at(&loser), None);
    assert_eq!(index.doctype_at(Path::new("/nowhere.json")), None);
}

#[test]
fn every_field_keeps_the_range_of_its_object_in_the_json() {
    let index = index();
    let doctype = index.doctype("Library Member").expect("Library Member");
    let source = std::fs::read_to_string(&doctype.path).expect("readable JSON");

    let field = index.field("Library Member", "referred_by").expect("field");
    let text = field.range.text(&source).expect("in bounds");
    assert!(text.starts_with('{') && text.ends_with('}'), "{text}");
    assert!(text.contains("\"fieldname\": \"referred_by\""));

    // specs/21: originSelectionRange covers the string interior only.
    let options = field.ranges.options.expect("options range");
    assert_eq!(options.text(&source), Some("Library Member"));
}

// --- Implicit fields ---------------------------------------------------------

#[test]
fn implicit_fields_are_known_but_are_not_declared_fields() {
    let index = index();

    for fieldname in ["name", "owner", "creation", "modified", "docstatus", "idx"] {
        assert!(
            index.has_field("Library Member", fieldname),
            "{fieldname} must never report as unknown"
        );
        assert_eq!(
            index.field("Library Member", fieldname),
            None,
            "{fieldname} has no range, so it is not a declared field"
        );
    }

    for fieldname in ["_user_tags", "_comments", "_assign", "_liked_by", "_seen"] {
        assert!(index.has_field("Library Member", fieldname), "{fieldname}");
    }
}

#[test]
fn only_a_child_doctype_carries_the_parent_trio() {
    let index = index();

    for fieldname in ["parent", "parentfield", "parenttype"] {
        assert!(
            index.has_field("Library Loan Item", fieldname),
            "{fieldname}"
        );
        assert!(
            !index.has_field("Library Member", fieldname),
            "{fieldname} on a parent DocType"
        );
    }
}

#[test]
fn field_names_lists_declared_fields_first_then_implicit_ones() {
    let index = index();
    let names = index.field_names("Library Loan Item");

    assert_eq!(&names[..3], ["book_title", "due_date", "loan_status"]);
    assert!(names.contains(&"parentfield"));
    assert!(names.contains(&"_seen"));
    assert_eq!(
        names.len(),
        3 + 16,
        "3 declared, 8 standard, 3 child, 5 optional"
    );

    // A DocType that does not exist has no fields — not the implicit ones.
    assert!(index.field_names("Libary Member").is_empty());
}

// --- Field semantics ---------------------------------------------------------

#[test]
fn options_mean_what_the_fieldtype_says_they_mean() {
    let index = index();

    let link = index.field("Library Member", "referred_by").expect("Link");
    assert_eq!(index.options(link), Options::LinkTarget("Library Member"));

    let table = index.field("Library Member", "loans").expect("Table");
    assert_eq!(
        index.options(table),
        Options::ChildDocType("Library Loan Item")
    );

    let dynamic = index
        .field("Library Member", "reference_name")
        .expect("Dynamic Link");
    assert_eq!(
        index.options(dynamic),
        Options::DynamicSource("reference_type")
    );

    let email = index.field("Library Member", "email_id").expect("Data");
    assert_eq!(index.options(email), Options::FormatHint("Email"));

    let currency = index
        .field("Library Membership", "amount")
        .expect("Currency");
    assert_eq!(index.options(currency), Options::CurrencyField("currency"));

    let select = index
        .field("Library Member", "reference_type")
        .expect("Select");
    assert_eq!(
        index.options(select),
        Options::Select {
            choices: vec!["Library Membership", "Library Loan Item"],
            optional: true,
        }
    );

    // `Open\nClosed` has no leading blank, so the field is not optional.
    let status = index.field("Loan", "status").expect("Select");
    assert_eq!(
        index.options(status),
        Options::Select {
            choices: vec!["Open", "Closed"],
            optional: false,
        }
    );
}

#[test]
fn a_dynamic_link_resolves_to_its_sibling_field() {
    let index = index();
    let doctype = index.doctype("Library Member").expect("Library Member");
    let dynamic = index
        .field("Library Member", "reference_name")
        .expect("Dynamic Link");

    let source = index
        .dynamic_link_source(doctype, dynamic)
        .expect("sibling exists");
    assert_eq!(index.fieldname(source), "reference_type");

    // The deliberately dangling one in the circulation app.
    let broken = index
        .doctype("Circulation Broken Field")
        .expect("Circulation Broken Field");
    let dangling = index
        .field("Circulation Broken Field", "dangling_dynamic")
        .expect("field");
    assert_eq!(index.dynamic_link_source(broken, dangling), None);
}

#[test]
fn fetch_from_resolves_both_halves_or_neither() {
    let index = index();
    let doctype = index.doctype("Library Member").expect("Library Member");
    let field = index
        .field("Library Member", "referred_by_email")
        .expect("field");

    let (link, target) = index.fetch_from(doctype, field);
    assert_eq!(index.fieldname(link.expect("link half")), "referred_by");
    assert_eq!(index.fieldname(target.expect("target half")), "email_id");

    // `good_link.no_such_field`: the link resolves, the target does not.
    let broken = index
        .doctype("Circulation Broken Field")
        .expect("Circulation Broken Field");
    let bad = index
        .field("Circulation Broken Field", "bad_fetch")
        .expect("field");
    let (link, target) = index.fetch_from(broken, bad);
    assert!(link.is_some(), "the link half is a real field");
    assert_eq!(target, None);

    // A field without `fetch_from` resolves to nothing at all.
    let plain = index.field("Library Member", "member_name").expect("field");
    assert_eq!(index.fetch_from(doctype, plain), (None, None));
}

// --- Layer merge -------------------------------------------------------------

#[test]
fn a_custom_field_is_appended_with_layer_two_provenance() {
    let index = index();

    let loyalty = index
        .field("Library Member", "loyalty_points")
        .expect("the Custom Field merged");
    assert_eq!(index.fieldtype(loyalty), "Int");
    assert_eq!(loyalty.provenance.layer, Layer::Fixture);
    assert!(
        loyalty
            .provenance
            .source
            .as_deref()
            .expect("a source file")
            .ends_with("fixtures/custom_field.json"),
        "hover has to be able to say which file it came from"
    );

    // `library/api.py` reads loyalty_points; that read is only clean because of
    // this merge — tests/fixtures/README.md calls it the load-bearing case.
    let doctype = index.doctype("Library Member").expect("Library Member");
    assert!(doctype.has_customizations());
}

#[test]
fn a_custom_field_lands_where_insert_after_puts_it() {
    let index = index();
    let names = index.field_names("Library Membership");
    let at = names
        .iter()
        .position(|name| *name == "renewal_of")
        .expect("renewal_of merged");

    assert_eq!(names[at - 1], "to_date", "insert_after: to_date");
}

#[test]
fn a_property_setter_overrides_one_property_and_nothing_else() {
    let index = index();

    let email = index.field("Library Member", "email_id").expect("field");
    assert!(email.reqd, "email_id.reqd is set by a Property Setter");
    assert_eq!(email.provenance.layer, Layer::Fixture);
    // Everything else about the field still comes from the schema.
    assert_eq!(index.fieldtype(email), "Data");
    assert_eq!(email.label.as_deref(), Some("Email"));

    // An empty `field_name` targets the DocType.
    let doctype = index.doctype("Library Member").expect("Library Member");
    assert_eq!(
        doctype.search_fields.as_deref(),
        Some("member_name,email_id")
    );
}

#[test]
fn a_field_no_layer_touched_stays_at_layer_one() {
    let index = index();
    let field = index.field("Library Member", "member_name").expect("field");

    assert_eq!(field.provenance.layer, Layer::Schema);
    assert_eq!(&*field.provenance.app, "library");
    assert!(
        field
            .provenance
            .source
            .as_deref()
            .expect("a source")
            .ends_with("library_member.json")
    );
}

// --- Conflicts ---------------------------------------------------------------

#[test]
fn a_doctype_defined_by_two_installed_apps_conflicts_and_the_later_app_wins() {
    let index = index();

    let duplicate = index
        .conflicts()
        .iter()
        .find(|conflict| conflict.code == ConflictCode::DuplicateDocType)
        .expect("duplicate-doctype");

    assert_eq!(duplicate.doctype, "Library Availability");
    assert_eq!(
        duplicate.paths.len(),
        2,
        "the diagnostic lands on both files"
    );
    assert!(
        duplicate
            .paths
            .iter()
            .any(|p| p.to_string_lossy().contains("/library/"))
    );
    assert!(
        duplicate
            .paths
            .iter()
            .any(|p| p.to_string_lossy().contains("/circulation"))
    );

    // circulation_app is installed after library, so its copy wins.
    let winner = index.doctype("Library Availability").expect("merged");
    assert_eq!(&*winner.app, "circulation_app");
    assert!(index.has_field("Library Availability", "branch"));
    assert!(!index.has_field("Library Availability", "available"));
}

#[test]
fn the_uninstalled_app_contributes_nothing_and_conflicts_with_nothing() {
    let index = index();

    // library-old redefines both of these; it is not installed.
    for name in ["Library Member", "Library Settings"] {
        let doctype = index.doctype(name).expect(name);
        assert_eq!(&*doctype.app, "library");
        assert!(
            !doctype.path.to_string_lossy().contains("library-old"),
            "{name} came from the uninstalled app"
        );
    }

    assert!(
        index
            .conflicts()
            .iter()
            .all(|conflict| conflict.doctype != "Library Member"
                || conflict.code != ConflictCode::DuplicateDocType),
        "an uninstalled app must not raise duplicate-doctype"
    );
}

#[test]
fn a_custom_field_colliding_with_a_schema_field_conflicts_rather_than_overriding() {
    let index = index();

    let collision = index
        .conflicts()
        .iter()
        .find(|conflict| conflict.code == ConflictCode::CustomFieldCollision)
        .expect("custom-field-collision");

    assert_eq!(collision.doctype, "Library Member");
    assert_eq!(collision.fieldname.as_deref(), Some("member_name"));
    assert!(collision.paths[0].ends_with("circulation/fixtures/custom_field.json"));

    // The schema field is untouched: the Custom Field did not override it.
    let field = index.field("Library Member", "member_name").expect("field");
    assert_eq!(field.provenance.layer, Layer::Schema);
    assert_eq!(&*field.provenance.app, "library");
}

// --- Controllers -------------------------------------------------------------

#[test]
fn a_controller_beside_the_json_is_found_with_its_methods() {
    let index = index();
    let doctype = index.doctype("Library Member").expect("Library Member");
    let controller = doctype.controller.as_ref().expect("a controller");

    assert_eq!(controller.class_name, "LibraryMember");
    assert!(controller.path.ends_with("library_member.py"));
    assert_eq!(
        controller.dotted(),
        "library.library.doctype.library_member.library_member.LibraryMember"
    );
    assert_eq!(
        controller
            .methods
            .iter()
            .map(|method| method.name.as_str())
            .collect::<Vec<_>>(),
        ["validate", "add_loan", "renew_all"]
    );
    assert!(controller.method("renew_all").expect("method").whitelisted);
    assert!(!controller.method("validate").expect("method").whitelisted);
}

#[test]
fn a_doctype_without_a_controller_file_has_none() {
    let index = index();

    // circulation's Library Availability wins the merge and has no `.py`.
    let winner = index.doctype("Library Availability").expect("merged");
    assert_eq!(winner.controller, None);

    let broken = index
        .doctype("Circulation Broken Meta")
        .expect("Circulation Broken Meta");
    assert_eq!(broken.controller, None);
}

// --- Whitelist registry ------------------------------------------------------

#[test]
fn the_whitelist_registry_holds_exactly_the_decorated_functions() {
    let index = index();
    let paths: Vec<&str> = index
        .whitelisted_methods()
        .map(|method| method.dotted.as_str())
        .collect();

    assert_eq!(
        paths,
        [
            "library.api.get_member_summary",
            "library.api.get_member_summary_v2",
            "library.api.list_issued_titles",
            "library.api.ping",
            "library.library.doctype.library_member.library_member.LibraryMember.renew_all",
        ]
    );

    let guest = index
        .whitelisted("library.api.list_issued_titles")
        .expect("registered");
    assert!(guest.allow_guest);
    assert_eq!(guest.methods, ["GET"]);

    let summary = index
        .whitelisted("library.api.get_member_summary")
        .expect("registered");
    assert!(!summary.allow_guest);
    assert_eq!(summary.parameters, ["member"]);
    assert!(summary.methods.is_empty(), "unset means every method");
}

#[test]
fn an_undecorated_function_is_absent_from_the_registry() {
    let index = index();

    // The near miss that makes `non-whitelisted-call` testable.
    assert_eq!(index.whitelisted("circulation.api.recalculate_fines"), None);
    assert_eq!(index.whitelisted("library.api.loan_period"), None);
    assert_eq!(index.whitelisted("library.api.nothing_here"), None);
}

// --- Dotted paths ------------------------------------------------------------

#[test]
fn every_path_kind_resolves_on_the_fixture_bench() {
    let index = index();

    let callable = index.resolve("library.events.validate_member", Kind::Callable);
    assert_eq!(callable.outcome, Outcome::Resolved);
    assert_eq!(
        callable.best().expect("candidate").symbol,
        SymbolKind::Function
    );

    let whitelisted = index.resolve("library.api.get_member_summary", Kind::Whitelisted);
    assert_eq!(whitelisted.outcome, Outcome::Resolved);

    let module = index.resolve("library.jinja_helpers", Kind::Module);
    assert_eq!(module.outcome, Outcome::Resolved);
    assert_eq!(module.best().expect("candidate").symbol, SymbolKind::Module);

    let class = index.resolve("library.overrides.CustomLibraryMember", Kind::Class);
    assert_eq!(class.outcome, Outcome::Resolved);
    assert_eq!(class.best().expect("candidate").symbol, SymbolKind::Class);

    let method = index.resolve(
        "library.overrides.MembershipMixin.recalculate",
        Kind::Method,
    );
    assert_eq!(method.outcome, Outcome::Resolved);
    assert_eq!(method.best().expect("candidate").symbol, SymbolKind::Method);
}

#[test]
fn a_package_directory_that_is_not_the_app_name_still_resolves() {
    let index = index();

    // The app is `circulation_app`; its package directory is `circulation`, and
    // the dotted path is spelled with the package.
    assert_eq!(
        index
            .resolve("circulation.api.before_loan", Kind::Callable)
            .outcome,
        Outcome::Resolved
    );
    assert_eq!(
        index
            .resolve("circulation_app.api.before_loan", Kind::Callable)
            .outcome,
        Outcome::Unresolved
    );
}

#[test]
fn a_nested_package_resolves_through_its_directories() {
    let index = index();

    let patch = index.resolve("library.patches.v1_0.rename_member_field", Kind::Module);
    assert_eq!(patch.outcome, Outcome::Resolved);
    assert!(
        patch
            .best()
            .expect("candidate")
            .path
            .ends_with("patches/v1_0/rename_member_field.py")
    );

    let package = index.resolve("library.patches.v1_0", Kind::Module);
    assert_eq!(package.outcome, Outcome::Resolved);
    assert!(
        package
            .best()
            .expect("candidate")
            .path
            .ends_with("patches/v1_0/__init__.py")
    );
}

#[test]
fn the_wrong_kind_is_reported_separately_from_not_existing() {
    let index = index();

    // hooks.py points override_doctype_class at a function; specs/23 calls that
    // wrong-path-kind, and it is only distinguishable from unresolvable-path
    // because the resolver found something.
    let wrong = index.resolve("circulation.api.mark_returned", Kind::Class);
    assert_eq!(wrong.outcome, Outcome::WrongKind);
    assert_eq!(wrong.candidates.len(), 1);
    assert_eq!(wrong.candidates[0].symbol, SymbolKind::Function);

    let missing = index.resolve("circulation.api.no_such_handler", Kind::Callable);
    assert_eq!(missing.outcome, Outcome::Unresolved);
    assert!(missing.candidates.is_empty());
}

#[test]
fn a_function_that_is_not_whitelisted_resolves_but_does_not_satisfy_the_kind() {
    let index = index();

    let call = index.resolve("circulation.api.recalculate_fines", Kind::Whitelisted);
    assert_eq!(call.outcome, Outcome::WrongKind);
    assert_eq!(
        call.candidates.len(),
        1,
        "the function itself is still a target"
    );

    let allowed = index.resolve("library.api.ping", Kind::Whitelisted);
    assert_eq!(allowed.outcome, Outcome::Resolved);
}

#[test]
fn nonsense_paths_resolve_to_nothing_rather_than_to_a_guess() {
    let index = index();

    for dotted in [
        "",
        ".",
        "no_such_app.api.thing",
        "library..api",
        "library.api.get_member_summary.extra.segments",
    ] {
        let resolution = index.resolve(dotted, Kind::Callable);
        assert_eq!(
            resolution.outcome,
            Outcome::Unresolved,
            "{dotted:?} resolved to {:?}",
            resolution.candidates
        );
    }

    // The app package itself is a module, so asking for a callable there is
    // wrong-kind rather than unresolvable — the two must stay distinguishable.
    assert_eq!(
        index.resolve("library", Kind::Callable).outcome,
        Outcome::WrongKind
    );
    assert_eq!(
        index.resolve("library", Kind::Module).outcome,
        Outcome::Resolved
    );
}

// --- indexStatus -------------------------------------------------------------

#[test]
fn an_index_that_has_not_built_reports_partial() {
    let index = Index::partial();
    let status = index.status();

    assert_eq!(status.confidence, Confidence::Partial);
    assert_eq!(status.doctypes, 0);
    assert_eq!(Confidence::Partial.as_str(), "partial");
    assert_eq!(Confidence::Static.as_str(), "static");
}

#[test]
fn a_directory_that_is_no_workspace_indexes_to_an_empty_graph() {
    let index = Index::build(Path::new(env!("CARGO_MANIFEST_DIR")));
    let status = index.status();

    assert_eq!(status.apps, 0);
    assert_eq!(status.doctypes, 0);
    assert_eq!(status.problems, 0);
    assert_eq!(status.frappe_version, None);
    assert_eq!(status.confidence, Confidence::Static);
}

// --- Golden snapshot ---------------------------------------------------------

#[test]
fn the_merged_graph_matches_its_snapshot() {
    // Reviewing this diff is how a behaviour change gets noticed — specs/32.
    insta::assert_snapshot!(index().fingerprint());
}
