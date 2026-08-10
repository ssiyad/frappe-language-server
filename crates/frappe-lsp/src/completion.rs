//! `textDocument/completion` — [20](../../../docs/specs/20-completion.md).
//!
//! Candidates come from the index; the work here is ranking and the edit range.
//! Editors sort by `sortText`, and with a few thousand DocTypes the ordering is
//! what decides whether the list is usable.

use frappe_index::doctype::Options;
use frappe_index::resolve::Target as Resolved;
use frappe_index::{Index, Layer};
use frappe_syntax::{Buffer, Role};
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, CompletionList,
    CompletionResponse, Documentation, MarkupContent, MarkupKind, Range, TextEdit,
};

use crate::analysis::Found;
use crate::capabilities::ClientSupport;
use crate::locations::LineIndex;

/// Spec 20's cap on an unfiltered list.
const MAX_ITEMS: usize = 1000;

/// Candidates for the position under the cursor.
#[must_use]
pub fn complete(
    index: &Index,
    buffer: &Buffer,
    found: &Found,
    support: ClientSupport,
    max_items: usize,
) -> Option<CompletionResponse> {
    let typed = found.slot.text.clone().unwrap_or_default();
    let edit_range = interior_range(buffer, found, support);

    let mut items = match found.slot.role {
        Role::DocType => doctypes(index, &typed),
        Role::Field | Role::ChildField => fields(index, found, &typed)?,
        Role::FormEvent => form_events(index, found, &typed)?,
        _ => return None,
    };

    // Ranking is by `sortText`; the label is left alone so the client's own
    // fuzzy matching still works on `salinv`.
    for (position, item) in items.iter_mut().enumerate() {
        item.sort_text = Some(format!("{position:05}"));
        item.filter_text = Some(item.label.clone());
        item.text_edit = edit_range.map(|range| {
            tower_lsp_server::ls_types::CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: item.label.clone(),
            })
        });
        if !support.label_details {
            item.label_details = None;
        }
    }

    // An empty prefix means the client must re-query as the user types;
    // a filtered set is complete as it stands (spec 20).
    let cap = max_items.min(MAX_ITEMS.max(max_items));
    let incomplete = typed.is_empty() && items.len() > cap;
    items.truncate(cap);

    Some(CompletionResponse::List(CompletionList {
        is_incomplete: incomplete,
        items,
    }))
}

fn doctypes(index: &Index, typed: &str) -> Vec<CompletionItem> {
    let lowered = typed.to_ascii_lowercase();
    let mut candidates: Vec<&frappe_index::DocType> = index
        .doctypes()
        .filter(|doctype| typed.is_empty() || doctype.name.to_ascii_lowercase().contains(&lowered))
        .collect();

    candidates.sort_by(|a, b| {
        prefix_rank(&a.name, &lowered)
            .cmp(&prefix_rank(&b.name, &lowered))
            // A child DocType is rarely the argument to `get_doc` (spec 20).
            .then(a.is_child.cmp(&b.is_child))
            .then(a.name.cmp(&b.name))
    });

    candidates
        .into_iter()
        .map(|doctype| CompletionItem {
            label: doctype.name.clone(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some(format!(
                "{} · {}",
                doctype.module.as_deref().unwrap_or("—"),
                doctype.app
            )),
            label_details: Some(CompletionItemLabelDetails {
                detail: None,
                description: Some(doctype.app.to_string()),
            }),
            ..Default::default()
        })
        .collect()
}

fn fields(index: &Index, found: &Found, typed: &str) -> Option<Vec<CompletionItem>> {
    // Which DocType's fields depends on what resolved, and a position whose
    // anchor never settled has no candidate set at all.
    let doctype = match &found.target {
        Resolved::Field { doctype, .. } | Resolved::UnknownField { doctype, .. } => {
            index.doctype(doctype)?
        }
        _ => return None,
    };

    let lowered = typed.to_ascii_lowercase();
    let mut items: Vec<(u8, String, CompletionItem)> = Vec::new();

    for field in &doctype.fields {
        let fieldname = index.fieldname(field);
        if !typed.is_empty() && !fieldname.to_ascii_lowercase().contains(&lowered) {
            continue;
        }
        let fieldtype = index.fieldtype(field);
        let detail = match index.options(field) {
            Options::LinkTarget(target) | Options::ChildDocType(target) => {
                format!("{fieldtype} → {target}")
            }
            _ => fieldtype.to_string(),
        };

        // Fields the form puts first are the ones a caller usually wants.
        let rank = if field.reqd { 0 } else { 1 };
        items.push((
            rank,
            fieldname.to_string(),
            CompletionItem {
                label: fieldname.to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(detail),
                label_details: Some(CompletionItemLabelDetails {
                    detail: field.label.clone().map(|label| format!(" {label}")),
                    description: Some(match field.provenance.layer {
                        Layer::Schema => fieldtype.to_string(),
                        Layer::Fixture | Layer::Site => format!("{fieldtype} · custom"),
                    }),
                }),
                ..Default::default()
            },
        ));
    }

    // Implicit fields last: they are always valid and almost never what is
    // being typed (spec 20).
    for implicit in doctype.implicit_fields() {
        if !typed.is_empty() && !implicit.fieldname.to_ascii_lowercase().contains(&lowered) {
            continue;
        }
        items.push((
            2,
            implicit.fieldname.to_string(),
            CompletionItem {
                label: implicit.fieldname.to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(format!("{} · implicit", implicit.fieldtype)),
                ..Default::default()
            },
        ));
    }

    items.sort_by(|a, b| {
        prefix_rank(&a.1, &lowered)
            .cmp(&prefix_rank(&b.1, &lowered))
            .then(a.0.cmp(&b.0))
            .then(a.1.cmp(&b.1))
    });

    Some(items.into_iter().map(|(_, _, item)| item).collect())
}

/// The keyspace of a `frappe.ui.form.on` handler map — spec 11's two
/// namespaces plus the table events, which is what makes the typo'd handler
/// avoidable rather than merely reportable.
fn form_events(index: &Index, found: &Found, typed: &str) -> Option<Vec<CompletionItem>> {
    let doctype = index.doctype(found.target.doctype()?)?;
    let lowered = typed.to_ascii_lowercase();
    let matches = |name: &str| typed.is_empty() || name.to_ascii_lowercase().contains(&lowered);

    let mut items = Vec::new();

    for event in frappe_syntax::generated::FORM_LIFECYCLE_EVENTS {
        if matches(event) {
            items.push(CompletionItem {
                label: event.to_string(),
                kind: Some(CompletionItemKind::EVENT),
                detail: Some("lifecycle".to_string()),
                ..Default::default()
            });
        }
    }

    for field in &doctype.fields {
        let fieldname = index.fieldname(field);
        if matches(fieldname) {
            items.push(CompletionItem {
                label: fieldname.to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(format!("on change · {}", index.fieldtype(field))),
                ..Default::default()
            });
        }
        // A table field also carries its row events.
        if matches!(index.options(field), Options::ChildDocType(_)) {
            for suffix in ["_add", "_remove"] {
                let name = format!("{fieldname}{suffix}");
                if matches(&name) {
                    items.push(CompletionItem {
                        label: name,
                        kind: Some(CompletionItemKind::EVENT),
                        detail: Some("table row".to_string()),
                        ..Default::default()
                    });
                }
            }
        }
    }

    Some(items)
}

/// Exact prefix matches first — spec 20's first ranking rule.
fn prefix_rank(name: &str, lowered_prefix: &str) -> u8 {
    if lowered_prefix.is_empty() || name.to_ascii_lowercase().starts_with(lowered_prefix) {
        0
    } else {
        1
    }
}

/// The edit replaces the string's contents, never its quotes (spec 20).
fn interior_range(buffer: &Buffer, found: &Found, support: ClientSupport) -> Option<Range> {
    let span = found.origin(buffer);
    let text = buffer.text();
    let index = LineIndex::new(&text, support.position_encoding);
    Some(Range {
        start: index.position(span.start),
        end: index.position(span.end),
    })
}

/// `completionItem/resolve`: the documentation body, built for one item rather
/// than for every candidate on every keystroke (spec 20).
#[must_use]
pub fn resolve(index: &Index, mut item: CompletionItem, markdown: bool) -> CompletionItem {
    if item.documentation.is_some() {
        return item;
    }

    let body = match item.kind {
        Some(CompletionItemKind::CLASS) => index
            .doctype(&item.label)
            .map(|doctype| doctype_documentation(index, doctype)),
        _ => None,
    };

    if let Some(body) = body {
        item.documentation = Some(if markdown {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: body,
            })
        } else {
            Documentation::String(crate::hover::as_plain_text(&body))
        });
    }
    item
}

fn doctype_documentation(index: &Index, doctype: &frappe_index::DocType) -> String {
    let mut out = format!(
        "**{}** — {} · {}\n\n{} fields",
        doctype.name,
        doctype.module.as_deref().unwrap_or("—"),
        doctype.app,
        doctype.fields.len()
    );
    if let Some(title) = &doctype.title_field {
        out.push_str(&format!("\nTitle field: `{title}`"));
    }
    let _ = index;
    out
}
