//! `textDocument/definition` — [21](../../../docs/specs/21-navigation.md).
//!
//! The motivating feature. Frappe resolves its own structure at runtime, so a
//! DocType name in a string is a dead end in every other tool; this turns it
//! into a link.

use frappe_index::Index;
use frappe_index::resolve::Target as Resolved;

use crate::analysis::Found;
use crate::locations::Target;

/// Where a resolved position leads, in the order spec 21 lists.
///
/// Plural on purpose: a DocType is three files, and letting the client
/// disambiguate beats the server picking one.
#[must_use]
pub fn targets(index: &Index, found: &Found) -> Vec<Target> {
    match &found.target {
        Resolved::DocType { name } => doctype_targets(index, name),
        Resolved::Field {
            doctype,
            fieldname,
            implicit,
        } => field_targets(index, doctype, fieldname, *implicit),
        Resolved::Method { doctype, name } => method_targets(index, doctype, name),
        Resolved::Path(resolution) => resolution
            .candidates
            .iter()
            .map(|candidate| {
                if candidate.name.is_empty() {
                    // A module: the file is the target.
                    Target::file(candidate.path.clone())
                } else {
                    Target::named(
                        candidate.path.clone(),
                        candidate.range,
                        candidate.selection_range,
                    )
                }
            })
            .collect(),
        // Nothing to point at, and a guess would be worse than silence.
        Resolved::DocumentMember { .. }
        | Resolved::UnknownDocType { .. }
        | Resolved::UnknownField { .. }
        | Resolved::Unresolvable => Vec::new(),
    }
}

/// Controller class, DocType JSON, form script — spec 21's order.
fn doctype_targets(index: &Index, name: &str) -> Vec<Target> {
    let Some(doctype) = index.doctype(name) else {
        return Vec::new();
    };
    let mut targets = Vec::new();

    if let Some(controller) = &doctype.controller {
        targets.push(Target::named(
            controller.path.clone(),
            controller.range,
            controller.name_range,
        ));
    }

    targets.push(Target::whole(doctype.path.to_path_buf(), doctype.range));

    // `<x>.js` beside the JSON. Bound by directory here; `doctype_js` binds
    // scripts from elsewhere and arrives with the hooks surface in 010.
    let script = doctype.path.with_extension("js");
    if script.is_file() {
        targets.push(Target::file(script));
    }

    targets
}

/// The field's object inside its own file — which is the fixture, not the
/// schema, for a Custom Field ([04](../../../docs/specs/04-metadata-index.md)).
fn field_targets(index: &Index, doctype: &str, fieldname: &str, implicit: bool) -> Vec<Target> {
    // An implicit field is declared in `frappe/model/__init__.py`, not in this
    // workspace's schema. Pointing at a line of frappe's source would be a
    // worse answer than none.
    if implicit {
        return Vec::new();
    }

    let Some(field) = index.field(doctype, fieldname) else {
        return Vec::new();
    };
    let Some(source) = field.provenance.source.as_deref() else {
        return Vec::new();
    };

    let selection = field.ranges.fieldname.unwrap_or(field.range);
    vec![Target::named(source.to_path_buf(), field.range, selection)]
}

fn method_targets(index: &Index, doctype: &str, name: &str) -> Vec<Target> {
    let Some(controller) = index.doctype(doctype).and_then(|d| d.controller.as_ref()) else {
        return Vec::new();
    };
    controller
        .methods
        .iter()
        .filter(|method| method.name == name)
        .map(|method| Target::named(controller.path.clone(), method.range, method.name_range))
        .collect()
}
