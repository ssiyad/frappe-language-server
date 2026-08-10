//! Anchor resolution — the join between extraction and the index.
//!
//! Stage two of the request flow in
//! [02](../../../docs/specs/02-architecture.md). `frappe-syntax` says *this
//! string sits in a DocType position*; this says whether that DocType exists,
//! what fields it has, and where the definition is. Extraction never consults
//! the index and presentation never touches the tree, so this is the only place
//! the two meet.
//!
//! Everything here refuses to guess. A position extraction left as
//! [`Anchor::Unknown`] with no text stays [`Target::Unresolvable`], because the
//! alternative is the false positive [23](../../../docs/specs/23-diagnostics.md)
//! exists to prevent.

use frappe_syntax::{Anchor, Binding, Match, Role, Slot, Span};

use crate::doctype::Options;
use crate::fieldtypes;
use crate::index::Index;
use crate::paths::{Kind, Resolution};

/// What a position turned out to name.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// A DocType in the index.
    DocType {
        /// Its name, exactly as the index holds it.
        name: String,
    },
    /// A field on a DocType.
    Field {
        /// The DocType the field hangs off.
        doctype: String,
        /// The fieldname.
        fieldname: String,
        /// Whether it comes from `frappe/model/__init__.py` rather than the
        /// schema — implicit fields are completable and MUST NOT be reported
        /// as unknown ([04](../../../docs/specs/04-metadata-index.md)).
        implicit: bool,
    },
    /// A dotted path, with everything it could mean.
    Path(Box<Resolution>),
    /// A controller method on a resolved DocType.
    Method {
        /// The DocType whose controller holds it.
        doctype: String,
        /// The method name.
        name: String,
    },
    /// A name in a DocType position that the index does not have.
    UnknownDocType {
        /// The name as written.
        name: String,
        /// Names differing only in case. Frappe is case-sensitive here, so
        /// these are a hint, never a silent resolution
        /// ([04](../../../docs/specs/04-metadata-index.md)).
        similar: Vec<String>,
    },
    /// A `Document` attribute or method reached by attribute access —
    /// `doc.flags`, `doc.save`. Valid Python, not a field, and never a
    /// diagnostic.
    DocumentMember {
        /// The attribute name.
        name: String,
    },
    /// A fieldname the resolved DocType does not have.
    UnknownField {
        /// The DocType it was resolved against.
        doctype: String,
        /// The fieldname as written.
        fieldname: String,
    },
    /// A `frappe.ui.form.on` object key — spec 11's two namespaces share one
    /// keyspace, and a key matching neither is the typo'd handler that
    /// silently never fires.
    FormEvent {
        /// The key as written.
        name: String,
        /// The DocType the handler is registered on.
        doctype: String,
        /// What the key turned out to be.
        kind: FormEventKind,
    },
    /// Nothing can be said: the argument was not a literal, or its anchor
    /// never settled. Downstream MUST stay silent.
    Unresolvable,
}

impl Target {
    /// The DocType this position hangs off, whether it names the DocType
    /// itself or a field on one. What `typeDefinition` navigates to
    /// ([21](../../../docs/specs/21-navigation.md)).
    #[must_use]
    pub fn doctype(&self) -> Option<&str> {
        match self {
            Self::DocType { name } => Some(name),
            Self::Field { doctype, .. }
            | Self::Method { doctype, .. }
            | Self::FormEvent { doctype, .. }
            | Self::UnknownField { doctype, .. } => Some(doctype),
            Self::Path(_)
            | Self::DocumentMember { .. }
            | Self::UnknownDocType { .. }
            | Self::Unresolvable => None,
        }
    }

    /// Whether this names something that exists.
    #[must_use]
    pub fn is_resolved(&self) -> bool {
        matches!(
            self,
            Self::DocType { .. } | Self::Field { .. } | Self::Method { .. }
        ) || matches!(self, Self::Path(resolution) if resolution.best().is_some())
    }
}

/// Which namespace a form event key falls into — spec 11.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormEventKind {
    /// A vendored lifecycle name: `refresh`, `validate`, `on_submit`.
    Lifecycle,
    /// A fieldname, which fires when that field changes.
    Field,
    /// `<table>_add`, `<table>_remove`, `before_<table>_remove`.
    TableEvent,
    /// Neither. The handler is never called, and nothing else reports it.
    Unknown,
}

/// One resolved position.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// Where it is in the buffer.
    pub span: Span,
    /// What the position meant syntactically.
    pub role: Role,
    /// What it names.
    pub target: Target,
}

/// Resolve every position in one match.
#[must_use]
pub fn resolve(index: &Index, found: &Match) -> Vec<Resolved> {
    // A `Table` row binding stops at the field; naming the child DocType is a
    // schema lookup, which is this side of the line.
    let receiver = found.receiver.as_ref().and_then(|binding| match binding {
        Binding::DocType(name) => Some(name.clone()),
        Binding::TableRow { doctype, fieldname } => child_doctype(index, doctype, fieldname),
    });

    // `doc.append(F, {…})`: the dict keys belong to the child DocType behind
    // the `Table` field named in the first slot, not to the receiver.
    let child = found
        .slots
        .iter()
        .find(|slot| slot.role == Role::Field)
        .and_then(|slot| slot.text.as_deref())
        .and_then(|fieldname| {
            let parent = anchored_doctype(found, receiver.as_deref())?;
            child_doctype(index, parent, fieldname)
        });

    found
        .slots
        .iter()
        .map(|slot| Resolved {
            span: slot.span,
            role: slot.role,
            target: target(index, found, slot, receiver.as_deref(), child.as_deref()),
        })
        .collect()
}

/// Resolve every match, in source order.
#[must_use]
pub fn resolve_all(index: &Index, matches: &[Match]) -> Vec<Resolved> {
    matches
        .iter()
        .flat_map(|found| resolve(index, found))
        .collect()
}

fn target(
    index: &Index,
    found: &Match,
    slot: &Slot,
    receiver: Option<&str>,
    child: Option<&str>,
) -> Target {
    match slot.role {
        Role::DocType => match slot.text.as_deref() {
            Some(name) => doctype_target(index, name),
            None => Target::Unresolvable,
        },

        // The slot's own anchor wins where it has one: the 4-tuple filter form
        // re-anchors to the DocType named in the tuple, not to the call's
        // ([10](../../../docs/specs/10-surface-python.md)).
        Role::Field => {
            let doctype = match &slot.anchor {
                Anchor::Field(doctype, _) => Some(doctype.as_str()),
                _ => anchored_doctype(found, receiver),
            };
            match (slot.text.as_deref(), doctype) {
                (Some(fieldname), Some(doctype)) => field_target(index, doctype, fieldname),
                _ => Target::Unresolvable,
            }
        }

        // Extraction leaves these `Unknown` on purpose: the child DocType is
        // only knowable here.
        Role::ChildField => match (slot.text.as_deref(), child) {
            (Some(fieldname), Some(child)) => field_target(index, child, fieldname),
            _ => Target::Unresolvable,
        },

        Role::Path(kind) => match slot.text.as_deref() {
            Some(dotted) => Target::Path(Box::new(index.resolve(dotted, path_kind(kind)))),
            None => Target::Unresolvable,
        },

        Role::Method => match (slot.text.as_deref(), anchored_doctype(found, receiver)) {
            (Some(name), Some(doctype)) => method_target(index, doctype, name),
            _ => Target::Unresolvable,
        },

        Role::FormEvent => match (slot.text.as_deref(), anchored_doctype(found, receiver)) {
            (Some(name), Some(doctype)) => form_event(index, doctype, name),
            _ => Target::Unresolvable,
        },

        // Hook keys resolve in the binary crate against the vendored registry,
        // and a translation argument names nothing at all.
        Role::HookKey | Role::Translation(_) | Role::Property => Target::Unresolvable,
    }
}

/// The DocType a match is about: its own anchor, else what its receiver is
/// bound to.
fn anchored_doctype<'a>(found: &'a Match, receiver: Option<&'a str>) -> Option<&'a str> {
    match &found.anchor {
        Anchor::DocType(name) => Some(name),
        Anchor::Field(doctype, _) => Some(doctype),
        Anchor::DottedPath(_) | Anchor::Unknown => receiver,
    }
}

fn doctype_target(index: &Index, name: &str) -> Target {
    if index.doctype(name).is_some() {
        return Target::DocType {
            name: name.to_string(),
        };
    }
    Target::UnknownDocType {
        name: name.to_string(),
        similar: index
            .case_insensitive_matches(name)
            .into_iter()
            .map(str::to_string)
            .collect(),
    }
}

fn field_target(index: &Index, doctype: &str, fieldname: &str) -> Target {
    // A DocType the index does not have cannot say anything about its fields;
    // reporting the field too would be a second error for one mistake.
    if index.doctype(doctype).is_none() {
        return Target::Unresolvable;
    }

    if index.field(doctype, fieldname).is_some() {
        return Target::Field {
            doctype: doctype.to_string(),
            fieldname: fieldname.to_string(),
            implicit: false,
        };
    }
    if index.has_field(doctype, fieldname) {
        return Target::Field {
            doctype: doctype.to_string(),
            fieldname: fieldname.to_string(),
            implicit: true,
        };
    }

    // Checked after the fields, so a DocType that really has a field called
    // `status` or `title` keeps it: members lose ties, never win them.
    if fieldtypes::is_document_member(fieldname) {
        return Target::DocumentMember {
            name: fieldname.to_string(),
        };
    }

    // The controller's own methods are reached the same way — `self.validate`,
    // `doc.calculate_totals`. A hook a subclass implements is not on the base
    // class, so the vendored list above cannot know about it.
    if let Target::Method { .. } = method_target(index, doctype, fieldname) {
        return Target::Method {
            doctype: doctype.to_string(),
            name: fieldname.to_string(),
        };
    }

    Target::UnknownField {
        doctype: doctype.to_string(),
        fieldname: fieldname.to_string(),
    }
}

/// Classify a `frappe.ui.form.on` key against its DocType.
///
/// The two namespaces share one keyspace, so a key is checked against both
/// before it can be called unknown ([11](../../../docs/specs/11-surface-javascript.md)).
fn form_event(index: &Index, doctype: &str, name: &str) -> Target {
    // A DocType the index does not have cannot say which of its fieldnames
    // this is, so nothing is said at all.
    if index.doctype(doctype).is_none() {
        return Target::Unresolvable;
    }

    let kind = if frappe_syntax::generated::is_form_lifecycle_event(name) {
        FormEventKind::Lifecycle
    } else if index.has_field(doctype, name) {
        FormEventKind::Field
    } else if frappe_syntax::generated::table_event(name)
        .is_some_and(|(fieldname, _)| index.has_field(doctype, fieldname))
    {
        FormEventKind::TableEvent
    } else {
        FormEventKind::Unknown
    };

    Target::FormEvent {
        name: name.to_string(),
        doctype: doctype.to_string(),
        kind,
    }
}

fn method_target(index: &Index, doctype: &str, name: &str) -> Target {
    // Counting the classes hooks.py contributes: a mixin's methods are on the
    // document at runtime, so reporting them unknown would be wrong (spec 04).
    if index.has_controller_method(doctype, name) {
        Target::Method {
            doctype: doctype.to_string(),
            name: name.to_string(),
        }
    } else {
        Target::Unresolvable
    }
}

/// The child DocType behind a `Table` field.
fn child_doctype(index: &Index, doctype: &str, fieldname: &str) -> Option<String> {
    let field = index.field(doctype, fieldname)?;
    match index.options(field) {
        Options::ChildDocType(child) => Some(child.to_string()),
        _ => None,
    }
}

fn path_kind(kind: frappe_syntax::PathKind) -> Kind {
    match kind {
        frappe_syntax::PathKind::Callable => Kind::Callable,
        frappe_syntax::PathKind::Whitelisted => Kind::Whitelisted,
        frappe_syntax::PathKind::Module => Kind::Module,
        frappe_syntax::PathKind::Class => Kind::Class,
        frappe_syntax::PathKind::Method => Kind::Method,
    }
}
