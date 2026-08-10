//! Positions inside `hooks.py` — [12](../../../docs/specs/12-surface-hooks.md).
//!
//! `hooks.py` is not a call surface, so extraction's shape tables have nothing
//! to say about it: the meaning of a string here comes from the hook it sits
//! under, and from where in that hook's shape it sits. A path under
//! `doc_events` is a handler; the same string under `doctype_js` is a file.

use std::path::{Path, PathBuf};

use frappe_index::Index;
use frappe_index::paths::Kind;
use frappe_syntax::generated::{self, Schema};
use frappe_syntax::hooks::{Assignment, Value};
use frappe_syntax::matches::Span;

/// What a position inside `hooks.py` names.
#[derive(Debug, Clone, PartialEq)]
pub enum HookTarget {
    /// The hook name itself.
    Key {
        /// The name as written.
        name: String,
        /// Its schema, or `None` for a name `frappe` never reads.
        schema: Option<Schema>,
    },
    /// A DocType, as a map key or a `fixtures` entry.
    DocType(String),
    /// A controller lifecycle event under `doc_events`.
    DocEvent {
        /// The event name.
        name: String,
        /// Whether it is one `frappe` fires.
        known: bool,
        /// The DocType it was registered against, when there is one.
        doctype: Option<String>,
    },
    /// A scheduler frequency.
    Frequency {
        /// The key as written.
        name: String,
        /// Whether the scheduler knows it.
        known: bool,
    },
    /// A cron expression under `scheduler_events.cron`.
    Cron {
        /// The expression.
        expression: String,
        /// Whether `croniter` would accept it.
        valid: bool,
    },
    /// A dotted path, with what the hook expects it to name.
    Path {
        /// The path as written.
        dotted: String,
        /// The kind the hook requires.
        kind: Kind,
    },
    /// An asset path, resolved against the app package.
    Asset {
        /// The path as written.
        written: String,
        /// The file, when it exists.
        file: Option<PathBuf>,
    },
}

/// A located hook position.
#[derive(Debug, Clone, PartialEq)]
pub struct Located {
    /// Where it is — the string interior for a literal, the name for a key.
    pub span: Span,
    /// What it names.
    pub target: HookTarget,
}

/// Whether a path is an app's `hooks.py`.
#[must_use]
pub fn is_hooks_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "hooks.py")
}

/// Resolve whatever sits at `offset` in a `hooks.py` buffer.
#[must_use]
pub fn at(index: &Index, source: &str, offset: usize, path: &Path) -> Option<Located> {
    let assignments = frappe_syntax::hooks::read(source).ok()?;
    let assignment = assignments
        .iter()
        .find(|assignment| assignment.span.contains(offset))?;

    if assignment.key_span.contains(offset) {
        return Some(Located {
            span: assignment.key_span,
            target: HookTarget::Key {
                name: assignment.key.clone(),
                schema: generated::hook_schema(&assignment.key),
            },
        });
    }

    let schema = generated::hook_schema(&assignment.key)?;
    let package = package_of(index, path);
    let app = app_of(index, path);
    within(&assignment.value, offset, schema, Depth::Top, None).map(|mut located| {
        // The asset case needs the package, which only the index knows.
        if let HookTarget::Asset { written, .. } = &located.target
            && let (Some(package), Some(app)) = (package.as_deref(), app.as_deref())
        {
            located.target = HookTarget::Asset {
                written: written.clone(),
                file: frappe_index::hooks::asset_path(package, app, written),
            };
        }
        located
    })
}

/// Every position in a `hooks.py`, for checks that read the whole file rather
/// than one cursor.
#[must_use]
pub fn all(index: &Index, source: &str, path: &Path) -> Vec<Located> {
    let Ok(assignments) = frappe_syntax::hooks::read(source) else {
        return Vec::new();
    };
    let package = package_of(index, path);
    let app = app_of(index, path);

    let mut found = Vec::new();
    for assignment in &assignments {
        let schema = generated::hook_schema(&assignment.key);
        found.push(Located {
            span: assignment.key_span,
            target: HookTarget::Key {
                name: assignment.key.clone(),
                schema,
            },
        });
        if let Some(schema) = schema {
            collect(&assignment.value, schema, Depth::Top, None, &mut found);
        }
    }

    for located in &mut found {
        if let HookTarget::Asset { written, .. } = &located.target
            && let (Some(package), Some(app)) = (package.as_deref(), app.as_deref())
        {
            located.target = HookTarget::Asset {
                written: written.clone(),
                file: frappe_index::hooks::asset_path(package, app, written),
            };
        }
    }
    found
}

/// The same walk as [`within`], collecting instead of stopping.
fn collect(
    value: &Value,
    schema: Schema,
    depth: Depth,
    doctype: Option<&str>,
    found: &mut Vec<Located>,
) {
    match value {
        Value::Text { text, span } => {
            found.push(leaf(text, interior(*span), schema, depth, doctype))
        }
        Value::List(items) => {
            for item in items {
                collect(item, schema, depth, doctype, found);
            }
        }
        Value::Map(entries) => {
            for (key, value) in entries {
                found.push(map_key(
                    key.text.as_str(),
                    interior(key.span),
                    schema,
                    depth,
                    doctype,
                ));
                let inner = if depth == Depth::Top && matches!(schema, Schema::DocEvents) {
                    Some(key.text.as_str())
                } else {
                    doctype
                };
                collect(value, schema, Depth::Inner, inner, found);
            }
        }
        Value::Scalar(_) | Value::Opaque(_) => {}
    }
}

/// How deep into a hook's shape a position is. The same string means different
/// things at different depths: a `doc_events` key is a DocType, its inner key
/// is an event, and the leaf is a handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Depth {
    Top,
    Inner,
}

fn within(
    value: &Value,
    offset: usize,
    schema: Schema,
    depth: Depth,
    doctype: Option<&str>,
) -> Option<Located> {
    match value {
        Value::Text { text, span } if span.contains(offset) => {
            Some(leaf(text, interior(*span), schema, depth, doctype))
        }
        Value::List(items) => items
            .iter()
            .find_map(|item| within(item, offset, schema, depth, doctype)),
        Value::Map(entries) => entries.iter().find_map(|(key, value)| {
            if key.span.contains(offset) {
                return Some(map_key(
                    key.text.as_str(),
                    interior(key.span),
                    schema,
                    depth,
                    doctype,
                ));
            }
            within(
                value,
                offset,
                schema,
                Depth::Inner,
                // `doc_events` keys its outer map by DocType, and the event
                // below it is only meaningful against that DocType.
                if depth == Depth::Top && matches!(schema, Schema::DocEvents) {
                    Some(key.text.as_str())
                } else {
                    doctype
                },
            )
        }),
        _ => None,
    }
}

/// A map key's meaning, which depends on the hook and the depth.
fn map_key(text: &str, span: Span, schema: Schema, depth: Depth, doctype: Option<&str>) -> Located {
    let target = match (schema, depth) {
        (Schema::DocEvents, Depth::Top)
        | (
            Schema::MapToClass | Schema::MapToClasses | Schema::MapToCallable | Schema::MapToFile,
            Depth::Top,
        ) => {
            // `"*"` is legal here and applies to every DocType; hover says so
            // rather than reporting it as a DocType nobody has.
            HookTarget::DocType(text.to_string())
        }
        (Schema::DocEvents, Depth::Inner) => HookTarget::DocEvent {
            name: text.to_string(),
            known: generated::is_doc_event(text),
            doctype: doctype.map(str::to_string),
        },
        (Schema::Scheduler, Depth::Top) => HookTarget::Frequency {
            name: text.to_string(),
            known: generated::is_frequency(text),
        },
        (Schema::Scheduler, Depth::Inner) => HookTarget::Cron {
            expression: text.to_string(),
            valid: generated::is_cron(text),
        },
        (Schema::MapPathToPath, _) => HookTarget::Path {
            dotted: text.to_string(),
            kind: Kind::Whitelisted,
        },
        _ => HookTarget::DocType(text.to_string()),
    };

    Located { span, target }
}

/// A leaf string's meaning.
fn leaf(text: &str, span: Span, schema: Schema, depth: Depth, doctype: Option<&str>) -> Located {
    let _ = (depth, doctype);
    let target = match schema {
        Schema::File | Schema::MapToFile => HookTarget::Asset {
            written: text.to_string(),
            file: None,
        },
        Schema::MapToClass | Schema::MapToClasses => HookTarget::Path {
            dotted: text.to_string(),
            kind: Kind::Class,
        },
        Schema::Jinja => HookTarget::Path {
            dotted: text.to_string(),
            // `jinja.methods` names modules, `jinja.filters` names functions;
            // Module resolution accepts both shapes without guessing.
            kind: Kind::Module,
        },
        Schema::Fixtures | Schema::AppNames => HookTarget::DocType(text.to_string()),
        Schema::Routes | Schema::Unknown => HookTarget::Path {
            dotted: text.to_string(),
            kind: Kind::Callable,
        },
        _ => HookTarget::Path {
            dotted: text.to_string(),
            kind: Kind::Callable,
        },
    };

    Located { span, target }
}

/// A literal's span without its quotes.
fn interior(span: Span) -> Span {
    Span::new(span.start + 1, span.end.saturating_sub(1))
}

fn package_of(index: &Index, path: &Path) -> Option<PathBuf> {
    index
        .apps()
        .iter()
        .find(|app| path.starts_with(&app.package))
        .map(|app| app.package.clone())
}

fn app_of(index: &Index, path: &Path) -> Option<String> {
    index
        .apps()
        .iter()
        .find(|app| path.starts_with(&app.package))
        .map(|app| app.name.clone())
}

/// Every assignment in a buffer, for callers that want the whole file.
#[must_use]
pub fn assignments(source: &str) -> Vec<Assignment> {
    frappe_syntax::hooks::read(source).unwrap_or_default()
}
