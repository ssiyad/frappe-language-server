//! Layer 3 in the graph — [05](../../../docs/specs/05-live-bridge.md).
//!
//! The bridge's transport is tested against a socket in `frappe-bridge`. This
//! is the other half: what the records do to the index once they arrive, and
//! what dropping them leaves behind.

use std::path::{Path, PathBuf};

use frappe_index::{Confidence, Index};

fn bench() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("tests/fixtures/bench")
}

/// A Custom Field row as `/api/v2` returns it, tagged the way the fixtures
/// reader dispatches on.
fn custom_fields(rows: &str) -> String {
    rows.to_string()
}

#[test]
fn a_field_added_through_customize_form_joins_the_doctype() {
    let mut index = Index::build(&bench());
    assert_eq!(index.confidence(), Confidence::Static);
    assert!(!index.has_field("Library Member", "shelf_preference"));

    let touched = index.apply_site(
        "library.localhost",
        &custom_fields(
            r#"[{"doctype":"Custom Field","name":"Library Member-shelf_preference",
                 "dt":"Library Member","fieldname":"shelf_preference",
                 "fieldtype":"Select","options":"Quiet\nBusy","label":"Shelf Preference"}]"#,
        ),
        "[]",
        &[],
    );

    assert_eq!(touched, ["Library Member"]);
    assert!(index.has_field("Library Member", "shelf_preference"));
    assert_eq!(index.confidence(), Confidence::Full);
    assert_eq!(index.site(), Some("library.localhost"));

    // It carries the site as its provenance, which is what hover reports.
    let field = index
        .field("Library Member", "shelf_preference")
        .expect("the field");
    assert_eq!(field.provenance.layer.as_str(), "site");
    assert_eq!(field.provenance.app.as_ref(), "library.localhost");
    // No file, which is exactly what eviction keys on.
    assert!(field.provenance.source.is_none());
}

#[test]
fn eviction_leaves_the_graph_as_it_was() {
    let mut index = Index::build(&bench());
    let before = index.fingerprint();

    index.apply_site(
        "library.localhost",
        r#"[{"doctype":"Custom Field","name":"x","dt":"Library Member",
             "fieldname":"shelf_preference","fieldtype":"Data"}]"#,
        "[]",
        &[],
    );
    assert_ne!(index.fingerprint(), before, "the field never landed");

    index.evict_site(true);
    assert_eq!(
        index.fingerprint(),
        before,
        "eviction left something behind"
    );
    assert!(!index.has_field("Library Member", "shelf_preference"));
    assert_eq!(index.site(), None);
}

#[test]
fn a_second_sync_drops_what_the_first_added() {
    // A field deleted through Customize Form must leave, or the graph keeps a
    // field the site no longer has and every later answer is wrong about it.
    let mut index = Index::build(&bench());
    index.apply_site(
        "library.localhost",
        r#"[{"doctype":"Custom Field","name":"x","dt":"Library Member",
             "fieldname":"first","fieldtype":"Data"}]"#,
        "[]",
        &[],
    );
    assert!(index.has_field("Library Member", "first"));

    index.apply_site(
        "library.localhost",
        r#"[{"doctype":"Custom Field","name":"y","dt":"Library Member",
             "fieldname":"second","fieldtype":"Data"}]"#,
        "[]",
        &[],
    );
    assert!(index.has_field("Library Member", "second"));
    assert!(
        !index.has_field("Library Member", "first"),
        "the first sync's field survived"
    );
}

#[test]
fn the_two_off_states_are_not_the_same_state() {
    // Spec 05's distinction: with no bridge the user accepted that
    // customizations are invisible, so warnings are still useful. With one
    // configured and down, the server knows its picture is incomplete.
    let mut index = Index::build(&bench());
    index.apply_site("library.localhost", "[]", "[]", &[]);
    assert_eq!(index.confidence(), Confidence::Full);

    index.evict_site(true);
    assert_eq!(index.confidence(), Confidence::Static);

    index.apply_site("library.localhost", "[]", "[]", &[]);
    index.evict_site(false);
    assert_eq!(index.confidence(), Confidence::Degraded);
}

#[test]
fn a_property_setter_from_the_site_overrides_the_schema() {
    let mut index = Index::build(&bench());
    let before = index
        .field("Library Member", "member_name")
        .expect("a schema field")
        .reqd;

    index.apply_site(
        "library.localhost",
        "[]",
        r#"[{"doctype":"Property Setter","name":"p","doc_type":"Library Member",
             "field_name":"member_name","property":"reqd","value":"1",
             "property_type":"Check"}]"#,
        &[],
    );

    let after = index
        .field("Library Member", "member_name")
        .expect("still there")
        .reqd;
    assert!(
        after,
        "the site's Property Setter did not apply (was {before})"
    );
}

/// A DocType's meta as `/api/v2/doctype/<name>/meta` returns it: the DocType
/// JSON shape, with nothing on disk behind it.
const LOYALTY_TIER: &str = r#"{"name":"Loyalty Tier","module":"Library","custom":1,
    "fields":[{"fieldname":"tier_name","fieldtype":"Data","label":"Tier"},
              {"fieldname":"member","fieldtype":"Link","options":"Library Member"}]}"#;

#[test]
fn a_doctype_created_through_the_ui_joins_the_index() {
    let mut index = Index::build(&bench());
    assert!(index.doctype("Loyalty Tier").is_none());

    let touched = index.apply_site(
        "library.localhost",
        r#"[{"doctype":"Custom Field","name":"Loyalty Tier-points","dt":"Loyalty Tier",
             "fieldname":"points","fieldtype":"Int"}]"#,
        "[]",
        &[LOYALTY_TIER.to_string()],
    );

    assert!(touched.contains(&"Loyalty Tier".to_string()), "{touched:?}");
    let tier = index.doctype("Loyalty Tier").expect("from the site");
    assert!(tier.is_site_only());
    assert_eq!(&*tier.app, "library.localhost");
    assert!(index.has_field("Loyalty Tier", "tier_name"));
    // A site's Custom Field on a site's DocType merges like any other.
    assert!(index.has_field("Loyalty Tier", "points"));
}

#[test]
fn an_installed_doctype_wins_over_the_sites_copy() {
    let mut index = Index::build(&bench());
    let installed = r#"{"name":"Library Member","module":"Library","custom":1,
        "fields":[{"fieldname":"only_on_the_site","fieldtype":"Data"}]}"#;
    index.apply_site("library.localhost", "[]", "[]", &[installed.to_string()]);

    let member = index.doctype("Library Member").expect("installed");
    assert!(!member.is_site_only());
    assert!(!index.has_field("Library Member", "only_on_the_site"));
}

#[test]
fn a_ui_doctype_leaves_with_the_sync_that_brought_it() {
    let mut index = Index::build(&bench());
    index.apply_site("library.localhost", "[]", "[]", &[LOYALTY_TIER.to_string()]);
    assert!(index.doctype("Loyalty Tier").is_some());

    // Deleted on the site: the next sync does not carry it.
    index.apply_site("library.localhost", "[]", "[]", &[]);
    assert!(index.doctype("Loyalty Tier").is_none());

    index.apply_site("library.localhost", "[]", "[]", &[LOYALTY_TIER.to_string()]);
    index.evict_site(false);
    assert!(index.doctype("Loyalty Tier").is_none());
}
