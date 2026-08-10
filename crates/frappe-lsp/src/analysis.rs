//! What is under the cursor — the entry point all three features share.
//!
//! The request flow of [02](../../../docs/specs/02-architecture.md) in one
//! function: locate the node, match the surface pattern, resolve the anchor.
//! Definition, hover and completion differ only in what they do with the
//! answer, so none of them repeats the first three steps.

use frappe_index::Index;
use frappe_index::resolve::{Resolved, Target, resolve};
use frappe_syntax::{Buffer, Context, Language, Match, Role, Slot, Span, extract};

/// A position, its surrounding call, and what it names.
#[derive(Debug, Clone)]
pub struct Found {
    /// The match the position sits in.
    pub matched: Match,
    /// The position itself.
    pub slot: Slot,
    /// What it resolved to.
    pub target: Target,
}

impl Found {
    /// The span to highlight: the string interior, never the quotes
    /// ([21](../../../docs/specs/21-navigation.md)).
    #[must_use]
    pub fn origin(&self, buffer: &Buffer) -> Span {
        interior(self.slot.span, buffer)
    }
}

/// Resolve whatever sits at `offset`.
///
/// `None` means the offset is not in a position this server knows about, which
/// is the common case: most of a Python file is not a Frappe surface.
#[must_use]
pub fn at(index: &Index, buffer: &Buffer, offset: usize, context: Context<'_>) -> Option<Found> {
    let matches = extract(buffer.language(), buffer.tree(), &buffer.text(), context);

    // Inner matches win: `doc.append("loans", {...})` nests child-field
    // positions inside the call's own span, and the narrower one is what the
    // cursor is actually in.
    let mut best: Option<(Match, Slot)> = None;
    for matched in matches {
        let Some(slot) = matched.slot_at(offset) else {
            continue;
        };
        let narrower = best
            .as_ref()
            .is_none_or(|(_, chosen)| span_len(slot.span) < span_len(chosen.span));
        if narrower {
            best = Some((matched.clone(), slot.clone()));
        }
    }

    let (matched, slot) = best?;
    let target = resolve(index, &matched)
        .into_iter()
        .find(|resolved: &Resolved| resolved.span == slot.span)
        .map(|resolved| resolved.target)
        .unwrap_or(Target::Unresolvable);

    Some(Found {
        matched,
        slot,
        target,
    })
}

/// The context a buffer needs to bind `self` — the DocType whose controller
/// this file is ([06](../../../docs/specs/06-analysis-pipeline.md)).
#[must_use]
pub fn context_for<'a>(index: &'a Index, path: Option<&std::path::Path>) -> Context<'a> {
    let controller = path.and_then(|path| controller_doctype(index, path));
    Context {
        self_doctype: controller.map(|doctype| doctype.name.as_str()),
        self_class: controller
            .and_then(|doctype| doctype.controller.as_ref())
            .map(|controller| controller.class_name.as_str()),
    }
}

fn controller_doctype<'a>(
    index: &'a Index,
    path: &std::path::Path,
) -> Option<&'a frappe_index::DocType> {
    index
        .doctypes()
        .find(|doctype| doctype.controller.as_ref().is_some_and(|c| c.path == path))
}

/// The interior of a string literal: the span without its quotes.
///
/// Extraction hands back the quoted node so a caller can offer completion
/// inside it; every user-visible range wants what is between the quotes
/// ([21](../../../docs/specs/21-navigation.md), [22](../../../docs/specs/22-hover.md)).
#[must_use]
pub fn interior(span: Span, buffer: &Buffer) -> Span {
    let text = buffer.slice(span.range());
    let bytes = text.as_bytes();

    let Some(&first) = bytes.first() else {
        return span;
    };
    if first != b'"' && first != b'\'' {
        return span;
    }

    // Prefixes (`f"…"`, `r'…'`) are stripped by extraction before the text is
    // reported, but the span still covers them.
    let closes = bytes.last() == Some(&first) && bytes.len() >= 2;
    Span::new(span.start + 1, if closes { span.end - 1 } else { span.end })
}

fn span_len(span: Span) -> usize {
    span.end.saturating_sub(span.start)
}

/// Whether a position is one completion has candidates for.
#[must_use]
pub fn completable(role: Role) -> bool {
    matches!(role, Role::DocType | Role::Field | Role::ChildField)
}

/// Whether this buffer is a surface the server reads.
///
/// DocType JSON is excluded on purpose: the index reads it with `serde_json`,
/// and the tree exists only for in-buffer positions
/// ([06](../../../docs/specs/06-analysis-pipeline.md)).
#[must_use]
pub fn supported(language: Language) -> bool {
    matches!(language, Language::Python | Language::JavaScript)
}
