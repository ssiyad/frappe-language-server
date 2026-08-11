//! `textDocument/definition` — [21](../../../docs/specs/21-navigation.md).
//!
//! The motivating feature. Frappe resolves its own structure at runtime, so a
//! DocType name in a string is a dead end in every other tool; this turns it
//! into a link.

use frappe_index::Index;
use frappe_index::resolve::Target as Resolved;

use crate::analysis::Found;
use crate::hooks::HookTarget;
use crate::locations::Target;
use frappe_index::paths::Kind;

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
        // A remapped path lands where the call actually goes at runtime, so
        // that is where navigation goes first; the original stays reachable
        // behind it ([21](../../../docs/specs/21-navigation.md)).
        Resolved::Path(resolution) => {
            let effective = index.effective_method(&resolution.dotted);
            if effective == resolution.dotted {
                resolution
                    .candidates
                    .iter()
                    .map(target_of)
                    .collect()
            } else {
                let mut targets = candidates(index, effective, Kind::Whitelisted);
                targets.extend(resolution.candidates.iter().map(target_of));
                targets
            }
        }
        // A form event names a vendored constant or a field; the field case
        // is reachable through the DocType, and neither is a location.
        Resolved::FormEvent { .. }
        // Nothing to point at, and a guess would be worse than silence.
        | Resolved::DocumentMember { .. }
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

    // The DocType's own controller, not whatever `override_doctype_class`
    // replaces it with: spec 21 puts those on `implementation`, which is the
    // question they answer.
    if let Some(controller) = &doctype.controller {
        targets.push(Target::named(
            controller.path.clone(),
            controller.range,
            controller.name_range,
        ));
    }

    targets.push(Target::whole(doctype.path.to_path_buf(), doctype.range));

    // `<x>.js` beside the JSON, bound by directory.
    let script = doctype.path.with_extension("js");
    if script.is_file() {
        targets.push(Target::file(script.clone()));
    }
    // …and anything `doctype_js` bound from elsewhere, which the directory
    // cannot show ([12](../../../docs/specs/12-surface-hooks.md)).
    for bound in index.scripts_for(name) {
        if bound != script {
            targets.push(Target::file(bound));
        }
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
    let mut targets = Vec::new();

    if let Some(controller) = index.doctype(doctype).and_then(|d| d.controller.as_ref()) {
        targets.extend(
            controller
                .methods
                .iter()
                .filter(|method| method.name == name)
                .map(|method| {
                    Target::named(controller.path.clone(), method.range, method.name_range)
                }),
        );
    }

    // A mixin's methods are on the document at runtime; the class is the best
    // target available without re-reading its file.
    targets.extend(
        index
            .controller_extras(doctype)
            .iter()
            .filter(|extra| extra.methods.iter().any(|method| method == name))
            .map(|extra| Target::named(extra.path.clone(), extra.range, extra.name_range)),
    );

    targets
}

/// Where a `hooks.py` position leads — [12](../../../docs/specs/12-surface-hooks.md).
#[must_use]
pub fn hook_targets(index: &Index, target: &HookTarget) -> Vec<Target> {
    match target {
        HookTarget::DocType(name) => doctype_targets(index, name),

        HookTarget::Path { dotted, kind } => {
            let effective = index.effective_method(dotted);
            let mut targets = candidates(index, effective, *kind);
            if effective != dotted {
                targets.extend(candidates(index, dotted, *kind));
            }
            targets
        }

        HookTarget::Asset { file, .. } => file
            .as_ref()
            .map(|path| vec![Target::file(path.clone())])
            .unwrap_or_default(),

        // A hook key's documentation is a URL, which hover carries; an event
        // name and a frequency are vendored constants with nowhere to go.
        // An app is a directory, and the bench is the only place to look.
        HookTarget::App(name) => index
            .apps()
            .iter()
            .find(|app| app.name == *name)
            .map(|app| vec![Target::file(app.package.join("hooks.py"))])
            .unwrap_or_default(),

        HookTarget::Text(_)
        | HookTarget::Key { .. }
        | HookTarget::DocEvent { .. }
        | HookTarget::Frequency { .. }
        | HookTarget::Cron { .. } => Vec::new(),
    }
}

/// `textDocument/implementation` on a DocType: the classes `hooks.py`
/// contributes to its controller — spec 21.
#[must_use]
pub fn implementations(index: &Index, doctype: &str) -> Vec<Target> {
    index
        .controller_extras(doctype)
        .iter()
        .map(|extra| Target::named(extra.path.clone(), extra.range, extra.name_range))
        .collect()
}

fn candidates(index: &Index, dotted: &str, kind: Kind) -> Vec<Target> {
    index
        .resolve(dotted, kind)
        .candidates
        .iter()
        .map(target_of)
        .collect()
}

/// A module has no symbol to select, so the file itself is the target.
fn target_of(candidate: &frappe_index::paths::Candidate) -> Target {
    if candidate.name.is_empty() {
        Target::file(candidate.path.clone())
    } else {
        Target::named(
            candidate.path.clone(),
            candidate.range,
            candidate.selection_range,
        )
    }
}
