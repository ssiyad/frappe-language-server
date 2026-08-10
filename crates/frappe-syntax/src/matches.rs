//! What extraction produces: `(pattern_id, spans)`, with the anchors syntax
//! alone can settle.
//!
//! [06](../../../docs/specs/06-analysis-pipeline.md) draws the line: this crate
//! recognizes call shapes and reads string literals out of them. It never
//! consults the index, so anything needing metadata — is this DocType real,
//! what child DocType does this `Table` field hold — stays open here.

use std::fmt;

/// A byte range in the buffer the match came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    /// First byte.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
}

impl Span {
    /// A span covering `start..end`.
    #[must_use]
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Whether `offset` falls inside the span.
    #[must_use]
    pub fn contains(&self, offset: usize) -> bool {
        (self.start..self.end).contains(&offset)
    }

    /// The span as a range, for slicing.
    #[must_use]
    pub fn range(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }
}

impl From<tree_sitter::Node<'_>> for Span {
    fn from(node: tree_sitter::Node<'_>) -> Self {
        Self::new(node.start_byte(), node.end_byte())
    }
}

/// What a position denotes.
///
/// `Unknown` is the load-bearing variant: it suppresses completion and
/// diagnostics downstream, and it is what every non-literal argument yields
/// ([06](../../../docs/specs/06-analysis-pipeline.md)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    /// A DocType name, from a literal or from a local binding.
    DocType(String),
    /// A fieldname on a DocType whose own anchor resolved first.
    Field(String, String),
    /// A dotted path — [07](../../../docs/specs/07-dotted-paths.md).
    DottedPath(String),
    /// Nothing can be said about this position.
    Unknown,
}

impl Anchor {
    /// Whether the anchor settled on something.
    #[must_use]
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown)
    }

    /// The DocType a field anchor hangs off, or the DocType itself.
    #[must_use]
    pub fn doctype(&self) -> Option<&str> {
        match self {
            Self::DocType(name) | Self::Field(name, _) => Some(name),
            Self::DottedPath(_) | Self::Unknown => None,
        }
    }
}

/// What a name is bound to inside one function scope.
///
/// The two cases of [06](../../../docs/specs/06-analysis-pipeline.md)'s binding
/// rules that survive without an index. Naming the child DocType behind a
/// `Table` field needs the schema, so [`Binding::TableRow`] stops at the field
/// and leaves the join to the resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    /// A literal DocType name from a recognized constructor, or `self` in a
    /// controller.
    DocType(String),
    /// A loop variable over `<doctype>.<fieldname>`, a `Table` field.
    TableRow {
        /// The DocType holding the `Table` field.
        doctype: String,
        /// The `Table` field being iterated.
        fieldname: String,
    },
}

impl Binding {
    /// The anchor this binding gives a position on the bound name.
    ///
    /// A `TableRow` yields `Unknown`: the child DocType is a schema lookup, and
    /// a guess here would be exactly the false positive
    /// [23](../../../docs/specs/23-diagnostics.md) forbids.
    #[must_use]
    pub fn anchor(&self) -> Anchor {
        match self {
            Self::DocType(name) => Anchor::DocType(name.clone()),
            Self::TableRow { .. } => Anchor::Unknown,
        }
    }
}

/// The identity of a recognized call shape, e.g. `frappe.db.get_value`.
///
/// Stable strings rather than an enum: the shape tables in
/// [10](../../../docs/specs/10-surface-python.md) and
/// [11](../../../docs/specs/11-surface-javascript.md) are data, and an enum
/// beside them would be a second copy to keep in step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PatternId(pub &'static str);

impl fmt::Display for PatternId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// What [07](../../../docs/specs/07-dotted-paths.md) expects a path to name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// Module-level `def` or `class`.
    Callable,
    /// `Callable` and decorated with `@frappe.whitelist()`.
    Whitelisted,
    /// A module, with no symbol segment.
    Module,
    /// A `class` statement.
    Class,
    /// `Class.method` on a resolved class.
    Method,
}

/// How a translation call was written.
///
/// `_()` and `__()` must receive a literal because strings are extracted
/// statically; only the tree can tell these apart
/// ([23](../../../docs/specs/23-diagnostics.md)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslationArg {
    /// A plain string literal.
    Literal,
    /// An f-string or a template literal with substitutions.
    Interpolated,
    /// `.format(...)` applied inside the call rather than outside.
    Formatted,
    /// String concatenation inside the call.
    Concatenated,
    /// Anything else — a name, a call, an expression.
    Dynamic,
}

/// What one captured position inside a match means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A DocType-name position.
    DocType,
    /// A fieldname resolved against the match's DocType anchor.
    Field,
    /// A fieldname on the child DocType behind a sibling `Table` field —
    /// `doc.append(F, {…})` and `frm.add_child(F, {…})`. Anchoring needs the
    /// schema, so these always carry [`Anchor::Unknown`].
    ChildField,
    /// A dotted path, with what the caller expects it to name.
    Path(PathKind),
    /// A method on the match's DocType anchor, named bare rather than as a
    /// dotted path — `frappe.enqueue_doc`'s third argument, `frm.call`'s
    /// `method`.
    Method,
    /// A `frappe.ui.form.on` object key —
    /// [11](../../../docs/specs/11-surface-javascript.md).
    FormEvent,
    /// A `hooks.py` key — [12](../../../docs/specs/12-surface-hooks.md).
    HookKey,
    /// The argument of `_()` / `__()`.
    Translation(TranslationArg),
    /// A DocField property name (`frm.set_df_property`).
    Property,
}

/// One captured position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// What the position means.
    pub role: Role,
    /// Where it is. For a string literal this is the quoted node, quotes
    /// included, so a caller can offer completion inside it.
    pub span: Span,
    /// The literal text, quotes and prefixes stripped. `None` when the
    /// argument was not a plain string literal.
    pub text: Option<String>,
    /// What the position denotes, as far as syntax settles it.
    pub anchor: Anchor,
}

impl Slot {
    /// A slot whose argument was not a literal, and so anchors to nothing.
    #[must_use]
    pub fn unknown(role: Role, span: Span) -> Self {
        Self {
            role,
            span,
            text: None,
            anchor: Anchor::Unknown,
        }
    }
}

/// One recognized call shape and the positions inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Which shape fired.
    pub pattern: PatternId,
    /// The whole call, decorator or object key.
    pub span: Span,
    /// The DocType the call is about, where syntax settles it.
    pub anchor: Anchor,
    /// For receiver shapes (`doc.get(F)`, `frm.set_value(F)`), what the
    /// receiver is bound to. `None` for free functions.
    pub receiver: Option<Binding>,
    /// The positions inside, in source order.
    pub slots: Vec<Slot>,
}

impl Match {
    /// The slot covering `offset`, if any — the request-flow entry point of
    /// [02](../../../docs/specs/02-architecture.md).
    #[must_use]
    pub fn slot_at(&self, offset: usize) -> Option<&Slot> {
        self.slots.iter().find(|slot| slot.span.contains(offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_row_binding_refuses_to_guess_the_child() {
        let binding = Binding::TableRow {
            doctype: "Library Member".into(),
            fieldname: "loans".into(),
        };
        assert_eq!(binding.anchor(), Anchor::Unknown);
    }

    #[test]
    fn a_doctype_binding_anchors_directly() {
        let binding = Binding::DocType("Library Member".into());
        assert_eq!(
            binding.anchor(),
            Anchor::DocType("Library Member".to_string())
        );
    }

    #[test]
    fn a_span_excludes_its_end() {
        let span = Span::new(4, 7);
        assert!(span.contains(4));
        assert!(span.contains(6));
        assert!(!span.contains(7));
        assert!(!span.contains(3));
    }
}
