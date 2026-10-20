//! `references`, `documentSymbol` and `workspaceSymbol` —
//! [21](../../../docs/specs/21-navigation.md).
//!
//! All three are presentation over structures the index already holds: the
//! reverse occurrence table, the merged DocType, and the artifact lists from
//! discovery. Nothing here reads the filesystem.

use std::path::Path;

use frappe_index::Range as ByteRange;
use frappe_index::occurrences::Key;
use frappe_index::{Index, resolve::Target};
use tower_lsp_server::ls_types::{
    DocumentSymbol, Location, OneOf, Range, SymbolKind, SymbolTag, WorkspaceSymbol,
};

use crate::analysis::Found;
use crate::locations::{LineIndex, uri_of};

/// Every use of the DocType or field under the cursor.
///
/// `None` rather than an empty list when the position names neither: an empty
/// result reads as "nothing uses this", which is a different and wrong answer.
#[must_use]
pub fn references(
    index: &Index,
    found: &Found,
    include_declaration: bool,
    encoding: frappe_syntax::PositionEncoding,
) -> Option<Vec<Location>> {
    let key = match &found.target {
        Target::DocType { name } => Key::DocType(name.clone()),
        Target::Field {
            doctype, fieldname, ..
        } => Key::Field(doctype.clone(), fieldname.clone()),
        _ => return None,
    };

    // Grouped by file, because the line index is per file: the sites arrive
    // sorted by path, so this is one pass and one index per file rather than
    // one per site.
    let sites = index.occurrences().get(&key);
    let mut locations = Vec::with_capacity(sites.len());
    for group in sites.chunk_by(|a, b| a.path == b.path) {
        let path = &group[0].path;
        let Some(uri) = uri_of(path) else {
            continue;
        };
        // The declaration is the JSON that defines it; spec 21 makes the
        // client's `includeDeclaration` decide whether it is in the list.
        let is_declaration = path
            .extension()
            .is_some_and(|extension| extension == "json");
        if is_declaration && !include_declaration {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };

        let lines = LineIndex::new(&source, encoding);
        locations.extend(group.iter().map(|site| Location {
            uri: uri.clone(),
            range: Range {
                start: lines.position(site.range.start as usize),
                end: lines.position(site.range.end as usize),
            },
        }));
    }
    Some(locations)
}

/// The outline of a DocType JSON, `hooks.py` or `patches.txt`.
///
/// A 200-field DocType JSON is otherwise unnavigable, which is the case this
/// exists for. Controller `.py` and `.js` get nothing: `documentSymbol` there
/// belongs to the general language server, and two servers answering means the
/// user sees each symbol twice.
#[must_use]
#[allow(deprecated)] // `DocumentSymbol::deprecated` is required by the struct.
pub fn document_symbols(
    index: &Index,
    path: &std::path::Path,
    source: &str,
    encoding: frappe_syntax::PositionEncoding,
) -> Vec<DocumentSymbol> {
    let lines = LineIndex::new(source, encoding);
    let range = |start: u32, end: u32| Range {
        start: lines.position(start as usize),
        end: lines.position(end as usize),
    };

    if path.file_name().is_some_and(|name| name == "patches.txt") {
        return patch_symbols(source, &lines);
    }
    if crate::hooks::is_hooks_file(path) {
        return hook_symbols(index, source, path, &lines);
    }

    let Some(doctype) = index.doctype_at(path) else {
        return Vec::new();
    };

    // Layout fields nest what follows them, which is what makes the outline
    // match the form the user sees rather than the file's flat array.
    let mut sections: Vec<DocumentSymbol> = Vec::new();
    let mut loose: Vec<DocumentSymbol> = Vec::new();
    for field in &doctype.fields {
        let fieldtype = index.fieldtype(field);
        let symbol = DocumentSymbol {
            name: index.fieldname(field).to_string(),
            detail: Some(fieldtype.to_string()),
            kind: SymbolKind::FIELD,
            tags: field.hidden.then(|| vec![SymbolTag::DEPRECATED]),
            deprecated: None,
            range: range(field.range.start, field.range.end),
            selection_range: range(
                field.ranges.fieldname.unwrap_or(field.range).start,
                field.ranges.fieldname.unwrap_or(field.range).end,
            ),
            children: None,
        };

        if matches!(fieldtype, "Section Break" | "Column Break" | "Tab Break") {
            sections.push(DocumentSymbol {
                kind: SymbolKind::NAMESPACE,
                name: field
                    .label
                    .clone()
                    .unwrap_or_else(|| index.fieldname(field).to_string()),
                children: Some(Vec::new()),
                ..symbol
            });
        } else if let Some(section) = sections.last_mut() {
            section.children.get_or_insert_with(Vec::new).push(symbol);
        } else {
            loose.push(symbol);
        }
    }

    let mut children = loose;
    children.append(&mut sections);
    vec![DocumentSymbol {
        name: doctype.name.clone(),
        detail: doctype.module.clone(),
        kind: SymbolKind::CLASS,
        tags: None,
        deprecated: None,
        range: range(0, u32::try_from(source.len()).unwrap_or(u32::MAX)),
        selection_range: range(0, 0),
        children: Some(children),
    }]
}

#[allow(deprecated)]
fn patch_symbols(source: &str, lines: &LineIndex<'_>) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim();
        let start = offset + (line.len() - line.trim_start().len());
        offset += line.len();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let end = start + trimmed.len();
        let range = Range {
            start: lines.position(start),
            end: lines.position(end),
        };
        symbols.push(DocumentSymbol {
            name: trimmed.to_string(),
            // `[post_model_sync]` is a section marker, not a patch.
            kind: if trimmed.starts_with('[') {
                SymbolKind::NAMESPACE
            } else {
                SymbolKind::FUNCTION
            },
            detail: None,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        });
    }
    symbols
}

#[allow(deprecated)]
fn hook_symbols(
    index: &Index,
    source: &str,
    path: &std::path::Path,
    lines: &LineIndex<'_>,
) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    for located in crate::hooks::all(index, source, path) {
        let crate::hooks::HookTarget::Key { name, .. } = &located.target else {
            continue;
        };
        let range = Range {
            start: lines.position(located.span.start),
            end: lines.position(located.span.end),
        };
        symbols.push(DocumentSymbol {
            name: name.clone(),
            detail: None,
            kind: SymbolKind::PROPERTY,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        });
    }
    symbols
}

/// Spec 21: capped at 200. A query matching half a bench is a query the user
/// will refine, and shipping 2 000 results makes every client slower for it.
const CAP: usize = 200;

/// DocTypes, whitelisted methods, reports, pages and web forms.
///
/// Fields are deliberately absent: 100 000 of them would drown every other
/// result, and they are reachable through their DocType.
///
/// Every location carries a range. A URI alone is legal only for a client
/// that advertises `workspaceSymbol/resolve`, and Claude Code, which does not,
/// fails on it. A file target starts at the top; a method is converted after
/// the cap, so a query reads at most [`CAP`] files.
#[must_use]
pub fn workspace_symbols(
    index: &Index,
    query: &str,
    encoding: frappe_syntax::PositionEncoding,
) -> Vec<WorkspaceSymbol> {
    let query = query.to_lowercase();
    let mut found: Vec<(_, WorkspaceSymbol, Option<(&Path, ByteRange)>)> = Vec::new();

    for doctype in index.doctypes() {
        if !fuzzy(&doctype.name.to_lowercase(), &query) {
            continue;
        }
        let Some(uri) = uri_of(&doctype.path) else {
            continue;
        };
        found.push((
            rank(&doctype.name.to_lowercase(), &query),
            WorkspaceSymbol {
                name: doctype.name.clone(),
                kind: SymbolKind::CLASS,
                tags: None,
                container_name: Some(match &doctype.module {
                    Some(module) => format!("{} · {module}", doctype.app),
                    None => doctype.app.to_string(),
                }),
                location: top_of(uri),
                data: None,
            },
            None,
        ));
    }

    for method in index.whitelisted_methods() {
        let name = method.dotted.rsplit('.').next().unwrap_or(&method.dotted);
        if !fuzzy(&name.to_lowercase(), &query) {
            continue;
        }
        let Some(uri) = uri_of(&method.path) else {
            continue;
        };
        found.push((
            rank(&name.to_lowercase(), &query),
            WorkspaceSymbol {
                name: method.dotted.clone(),
                kind: SymbolKind::METHOD,
                tags: None,
                container_name: method.owner.clone(),
                location: top_of(uri),
                data: None,
            },
            Some((method.path.as_path(), method.name_range)),
        ));
    }

    for app in index.apps() {
        for kind in ["Report", "Page", "Web Form"] {
            for record in app.artifacts.of_kind(kind) {
                if !fuzzy(&record.name.to_lowercase(), &query) {
                    continue;
                }
                let Some(uri) = uri_of(&record.path) else {
                    continue;
                };
                found.push((
                    rank(&record.name.to_lowercase(), &query),
                    WorkspaceSymbol {
                        name: record.name.clone(),
                        kind: SymbolKind::OBJECT,
                        tags: None,
                        container_name: Some(format!("{} · {kind}", app.name)),
                        location: top_of(uri),
                        data: None,
                    },
                    None,
                ));
            }
        }
    }

    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    found.truncate(CAP);
    found
        .into_iter()
        .map(|(_, mut symbol, name)| {
            if let Some((path, span)) = name
                && let Ok(text) = std::fs::read_to_string(path)
                && let OneOf::Left(location) = &mut symbol.location
            {
                location.range = LineIndex::new(&text, encoding).range(span);
            }
            symbol
        })
        .collect()
}

/// The start of a file: a valid range in every position encoding.
fn top_of(
    uri: tower_lsp_server::ls_types::Uri,
) -> OneOf<Location, tower_lsp_server::ls_types::WorkspaceLocation> {
    OneOf::Left(Location {
        uri,
        range: Range::default(),
    })
}

/// Subsequence match, the shape every editor's symbol box expects: `sinv`
/// finds `Sales Invoice`.
fn fuzzy(candidate: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut chars = candidate.chars();
    query
        .chars()
        .all(|wanted| chars.any(|actual| actual == wanted))
}

/// Lower sorts first: an exact prefix beats a scattered subsequence.
fn rank(candidate: &str, query: &str) -> (u8, usize) {
    let bucket = if candidate == query {
        0
    } else if candidate.starts_with(query) {
        1
    } else if candidate.contains(query) {
        2
    } else {
        3
    };
    (bucket, candidate.len())
}
