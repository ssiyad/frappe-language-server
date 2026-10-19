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

/// Like [`at`], for a line the user is still typing.
///
/// `frappe.get_doc("Sales Inv` has no closing quote or paren yet, so the tree
/// holds an `ERROR` where the call should be and nothing matches — and that is
/// exactly when completion is asked. This retries on copies of the buffer closed
/// at the cursor, and only when the line before the cursor shows an unclosed
/// string or a trailing `.name`, so an ordinary position costs nothing extra.
///
/// The slot is clipped to the cursor: what was inserted to close the line is
/// not text the user typed, and must reach neither the filter nor the edit.
#[must_use]
pub fn at_while_typing(
    index: &Index,
    buffer: &Buffer,
    offset: usize,
    context: Context<'_>,
) -> Option<Found> {
    let text = buffer.text();
    let offset = offset.min(text.len());
    let line = &text[text[..offset].rfind('\n').map_or(0, |newline| newline + 1)..offset];

    // The line's own closers first: in the middle of a file, what follows the
    // cursor closes the enclosing blocks. At the end of one, nothing does, and
    // every bracket still open before the cursor is closed too.
    let enclosing = still_open(&text[..offset], buffer.language());
    let attempts = closers(line)
        .into_iter()
        .flat_map(|closer| [closer.to_string(), format!("{closer}{enclosing}")])
        .filter(|attempt| !attempt.is_empty());
    for closer in attempts {
        let mut repaired = text.clone();
        repaired.insert_str(offset, &closer);
        let Ok(copy) = Buffer::open(buffer.language(), &repaired, 0, buffer.encoding()) else {
            return None;
        };
        let Some(mut found) = at(index, &copy, offset, context) else {
            continue;
        };
        if !completable(found.slot.role) || found.slot.span.start > offset {
            continue;
        }
        let start = interior(found.slot.span, &copy).start.min(offset);
        found.slot.span = Span::new(found.slot.span.start, offset);
        found.slot.text = Some(text[start..offset].to_string());
        return Some(found);
    }
    None
}

/// What might close the line, most likely first. Empty when nothing on the line
/// suggests the user is mid-token.
fn closers(line: &str) -> Vec<&'static str> {
    let unclosed = |quote: char| {
        let mut open = false;
        let mut escaped = false;
        for c in line.chars() {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote {
                open = !open;
            }
        }
        open
    };
    if unclosed('"') {
        return vec!["\"", "\")", "\"])", "\"})", "\": 1})", "\"]", "\"}"];
    }
    if unclosed('\'') {
        return vec!["'", "')", "'])", "'})", "': 1})", "']", "'}"];
    }
    // `frm.doc.` and `doc.sta`: an attribute with nothing, or half a name,
    // after the dot. The placeholder is clipped away again by the caller.
    let tail = line.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_');
    if tail.ends_with('.') {
        return vec!["x", "x)", "x;"];
    }
    Vec::new()
}

/// The closers for every bracket open before `prefix` ends, innermost first.
///
/// Strings and comments are skipped so a `(` inside one opens nothing. The
/// string the cursor sits in, if any, is the caller's to close.
fn still_open(prefix: &str, language: Language) -> String {
    let mut stack = Vec::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut chars = prefix.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == open || (c == '\n' && open != '`') {
                quote = None;
            }
            continue;
        }
        let comment = match language {
            Language::Python => c == '#',
            _ => c == '/' && chars.peek() == Some(&'/'),
        };
        if comment {
            while chars.next_if(|next| *next != '\n').is_some() {}
            continue;
        }
        match c {
            '"' | '\'' | '`' => quote = Some(c),
            '(' => stack.push(')'),
            '[' => stack.push(']'),
            '{' => stack.push('}'),
            ')' | ']' | '}' => {
                stack.pop();
            }
            _ => {}
        }
    }
    stack.into_iter().rev().collect()
}

/// The context a buffer needs to bind `self` — the DocType whose controller
/// this file is ([06](../../../docs/specs/06-analysis-pipeline.md)).
#[must_use]
pub fn context_for<'a>(index: &'a Index, path: Option<&std::path::Path>) -> Context<'a> {
    let controller = path.and_then(|path| controller_doctype(index, path));
    Context {
        frm_doctype: path.and_then(|path| index.doctype_bound_to(path)),
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
