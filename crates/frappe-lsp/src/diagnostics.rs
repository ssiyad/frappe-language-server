//! `textDocument/publishDiagnostics` — [23](../../../docs/specs/23-diagnostics.md).
//!
//! The riskiest feature in the project, and the first that speaks unprompted
//! about code somebody else wrote. Spec 23's governing rule is the whole
//! design: **report only what is certainly wrong**, and every check must
//! justify why it cannot fire on valid code. When in doubt, emit nothing.
//!
//! What is implemented here is the half of the check set that cannot be wrong
//! about correct code: a file that does not parse, and names decided by a
//! vendored table. Everything that judges a user's own naming — unknown
//! DocTypes and fields, non-whitelisted calls, translations — waits for the
//! negative-case sweep the fixture bench exists for.

use std::path::Path;

use frappe_index::{Confidence, Index};
use frappe_syntax::generated;
use tower_lsp_server::ls_types::{
    CodeDescription, Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range, Uri,
};

use frappe_index::doctype::Options;

use crate::config::{Config, Level};
use crate::hooks::HookTarget;
use crate::locations::LineIndex;

/// The prefix every code carries — spec 23.
const PREFIX: &str = "frappe-lsp";

/// Where a reader can look the code up.
const DOCS: &str =
    "https://github.com/ssiyad/frappe-language-server/blob/master/docs/specs/23-diagnostics.md";

/// A finding, before it becomes a protocol type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The code, without the `frappe-lsp/` prefix.
    pub code: &'static str,
    /// Byte range in the file.
    pub span: frappe_syntax::Span,
    /// What is wrong, in the words a user needs.
    pub message: String,
}

/// Everything wrong with one open buffer.
///
/// `source` is the buffer's text rather than the file's: the user is editing,
/// and a diagnostic on stale text points at the wrong line.
#[must_use]
pub fn for_buffer(index: &Index, config: &Config, path: &Path, source: &str) -> Vec<Diagnostic> {
    if !config.diagnostics.enabled {
        return Vec::new();
    }

    // A file in an app the index excluded — uninstalled, or outside the bench
    // — is one the server knows nothing about. Every path in it would resolve
    // to nothing, and every diagnostic would be an artefact of not looking
    // ([03](../../../docs/specs/03-workspace-model.md)).
    if !index.apps().is_empty()
        && index
            .workspace()
            .detection
            .bench_root()
            .is_some_and(|root| path.starts_with(root))
        && !index
            .apps()
            .iter()
            .any(|app| path.starts_with(&app.package))
    {
        return Vec::new();
    }

    let mut findings = Vec::new();
    if path.file_name().is_some_and(|name| name == "patches.txt") {
        findings.extend(patches(index, source));
    } else if crate::hooks::is_hooks_file(path) {
        findings.extend(hooks(index, source, path));
    } else if let Some(language) = frappe_syntax::Language::from_path(path)
        && crate::analysis::supported(language)
    {
        findings.extend(code(index, language, path, source));
    }
    findings.extend(schema(index, path));
    findings.extend(malformed(index, path));
    findings.extend(conflicts(index, path));

    let lines = LineIndex::new(source, frappe_syntax::PositionEncoding::Utf16);
    let knows_absence = absence_is_knowable(index);
    findings
        .into_iter()
        .filter(|finding| knows_absence || !asserts_absence(finding.code))
        .filter(|finding| severity(config, finding.code).is_some())
        .filter(|finding| !suppressed(source, &lines, finding))
        .map(|finding| {
            let range = Range {
                start: lines.position(finding.span.start),
                end: lines.position(finding.span.end),
            };
            Diagnostic {
                range,
                severity: severity(config, finding.code),
                code: Some(NumberOrString::String(format!("{PREFIX}/{}", finding.code))),
                code_description: DOCS
                    .parse::<Uri>()
                    .ok()
                    .map(|href| CodeDescription { href }),
                source: Some(PREFIX.to_string()),
                message: finding.message,
                ..Default::default()
            }
        })
        .collect()
}

/// Checks over `hooks.py`.
///
/// Every one is decided by a vendored table or by the shape of the file, so
/// none can be wrong about code that works — provided the table matches the
/// bench, which is what the version gate below is for.
fn hooks(index: &Index, source: &str, path: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let gated = version_matches(index);

    for located in crate::hooks::all(index, source, path) {
        match &located.target {
            HookTarget::Key { name, schema } if schema.is_none() && gated => {
                // `hooks.py` is an ordinary Python module and holds ordinary
                // Python names. A leading underscore or ALL_CAPS says the
                // author meant a constant, not a hook, and reporting those
                // would accuse every helper in the file.
                //
                // Edit distance is not enough on its own here: `doctype_js`
                // written as `doctype_javascript` is eight edits away and is
                // exactly the mistake worth catching. So the rule is the
                // convention, and the suggestion is offered when there happens
                // to be one.
                if !is_private_name(name) && !index.reads_hook(name) {
                    let message = match near_miss(name) {
                        Some(suggestion) => format!(
                            "`{name}` is not a hook frappe reads; did you mean `{suggestion}`?"
                        ),
                        None => {
                            format!("`{name}` is not a hook frappe reads; setting it does nothing")
                        }
                    };
                    findings.push(Finding {
                        code: "unknown-hook",
                        span: located.span,
                        message,
                    });
                }
            }
            HookTarget::DocEvent {
                name,
                known,
                doctype,
            } => {
                if !known && gated {
                    findings.push(Finding {
                        code: "unknown-doc-event",
                        span: located.span,
                        message: format!(
                            "`{name}` is not a document event; the handler never runs"
                        ),
                    });
                } else if let Some(doctype) = doctype
                    && generated::is_submit_only(name)
                    && index.doctype(doctype).is_some_and(|d| !d.is_submittable)
                {
                    // Not version-gated: this follows from the DocType on disk,
                    // not from a vendored list.
                    findings.push(Finding {
                        code: "non-submittable-event",
                        span: located.span,
                        message: format!("`{doctype}` is not submittable, so `{name}` never fires"),
                    });
                }
            }
            HookTarget::Frequency { name, known } if !known && gated => {
                findings.push(Finding {
                    code: "unknown-frequency",
                    span: located.span,
                    message: format!("`{name}` is not a scheduler frequency; nothing runs"),
                });
            }
            // A dotted path that names nothing, or names the wrong kind of
            // thing. Both follow from the workspace, not from a vendored list.
            HookTarget::Path { dotted, kind } => {
                use frappe_index::paths::{Kind, Outcome};
                let effective = index.effective_method(dotted);
                let outcome = index.resolve(effective, *kind).outcome;

                // A hook's declared kind is not always the only one that works:
                // `doc_events` takes a function or a `Class.method`, and
                // `jinja` takes a module of methods or a single filter. Report
                // only what no reading accepts.
                let accepted = |alternative| {
                    matches!(
                        index.resolve(effective, alternative).outcome,
                        Outcome::Resolved
                    )
                };
                match outcome {
                    // A module that resolves but has no such symbol is not
                    // proof of absence: the reader sees top-level definitions,
                    // and `frappe.utils.markdown` is imported into
                    // `frappe/utils/__init__.py` from elsewhere. Only a path
                    // whose *module* is missing is certainly wrong.
                    Outcome::Unresolved
                        if !accepted(Kind::Callable)
                            && !accepted(Kind::Module)
                            && !accepted(Kind::Method)
                            && !module_exists(index, effective) =>
                    {
                        findings.push(Finding {
                            code: "unresolvable-path",
                            span: located.span,
                            message: format!("`{dotted}` does not resolve in this workspace"),
                        });
                    }
                    // Only where the kind is the point: a class hook given a
                    // function silently fails to override anything.
                    Outcome::WrongKind if matches!(kind, Kind::Class) => findings.push(Finding {
                        code: "wrong-path-kind",
                        span: located.span,
                        message: format!("`{dotted}` is not a class"),
                    }),
                    // `Unknown` is decorator indirection, and MUST NOT report.
                    _ => {}
                }
            }

            HookTarget::Asset { written, file } if file.is_none() => findings.push(Finding {
                code: "missing-asset",
                span: located.span,
                message: format!("`{written}` is not a file in this app"),
            }),

            HookTarget::App(name) if !index.apps().iter().any(|app| app.name == *name) => {
                // Warning only: it may legitimately be installed elsewhere
                // ([12](../../../docs/specs/12-surface-hooks.md)).
                findings.push(Finding {
                    code: "unknown-required-app",
                    span: located.span,
                    message: format!("`{name}` is not installed in this bench"),
                });
            }

            HookTarget::DocType(name) if name != "*" && gated && index.doctype(name).is_none() => {
                findings.push(Finding {
                    code: "unknown-doctype",
                    span: located.span,
                    message: format!("no DocType named `{name}`"),
                });
            }

            // Cron syntax is checkable without a version — spec 23.
            HookTarget::Cron { expression, valid } if !valid => {
                findings.push(Finding {
                    code: "invalid-cron",
                    span: located.span,
                    message: format!(
                        "`{expression}` is not a valid 5-field cron expression; the scheduler raises on it at sync time"
                    ),
                });
            }
            _ => {}
        }
    }

    // `app_name` has to be the package directory: frappe resolves every dotted
    // path in the app through it, so a mismatch breaks the app at runtime.
    if let Some(app) = index
        .apps()
        .iter()
        .find(|app| path.starts_with(&app.package))
        && let Some(directory) = app.package.file_name().and_then(|name| name.to_str())
        && app.name != directory
        && let Some(assignment) = crate::hooks::all(index, source, path).into_iter().find(
            |located| matches!(&located.target, HookTarget::Key { name, .. } if name == "app_name"),
        )
    {
        findings.push(Finding {
            code: "app-name-mismatch",
            span: assignment.span,
            message: format!(
                "app_name is `{}` but the package directory is `{directory}`",
                app.name
            ),
        });
    }

    // Duplicate assignment: the second silently wins.
    let app = index
        .apps()
        .iter()
        .find(|app| path.starts_with(&app.package))
        .map(|app| app.name.clone());
    if let Some(hooks) = app.as_deref().and_then(|app| index.hooks(app)) {
        for duplicate in hooks.duplicates() {
            for assignment in hooks
                .assignments
                .iter()
                .filter(|assignment| assignment.key == duplicate)
                .skip(1)
            {
                findings.push(Finding {
                    code: "duplicate-hook",
                    span: assignment.key_span,
                    message: format!("`{duplicate}` is assigned more than once; the last wins"),
                });
            }
        }
    }

    findings
}

/// Whether the module half of a dotted path resolves, whatever the symbol on
/// the end of it turns out to be.
fn module_exists(index: &Index, dotted: &str) -> bool {
    let Some((module, _)) = dotted.rsplit_once('.') else {
        return false;
    };
    matches!(
        index
            .resolve(module, frappe_index::paths::Kind::Module)
            .outcome,
        frappe_index::paths::Outcome::Resolved
    )
}

/// Whether a name follows Python's conventions for something private or
/// constant, which a hook never is.
fn is_private_name(name: &str) -> bool {
    name.starts_with('_') || name.chars().all(|c| !c.is_lowercase())
}

/// The hook `name` was probably a typo of, if any.
///
/// Edit distance 2 over a 130-name table, which is small enough to scan and
/// tight enough that unrelated identifiers never match.
fn near_miss(name: &str) -> Option<&'static str> {
    generated::HOOKS
        .iter()
        .map(|(hook, _)| *hook)
        .filter(|hook| hook.len().abs_diff(name.len()) <= 2)
        .map(|hook| (distance(name, hook), hook))
        .filter(|(distance, _)| *distance <= 2)
        .min_by_key(|(distance, hook)| (*distance, *hook))
        .map(|(_, hook)| hook)
}

/// Levenshtein distance, capped by the caller's filter.
fn distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];

    for (row, from) in left.chars().enumerate() {
        current[0] = row + 1;
        for (column, to) in right.iter().enumerate() {
            let cost = usize::from(from != *to);
            current[column + 1] = (current[column] + 1)
                .min(previous[column + 1] + 1)
                .min(previous[column] + cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

/// Checks over a Python or JavaScript buffer.
///
/// Every one reads a resolved position, and resolution already answers
/// `Unresolvable` for anything it could not settle — so the discipline that
/// keeps these quiet is the same discipline the features rely on, rather than a
/// second set of rules to keep honest.
fn code(
    index: &Index,
    language: frappe_syntax::Language,
    path: &Path,
    source: &str,
) -> Vec<Finding> {
    let Ok(mut parser) = language.parser() else {
        return Vec::new();
    };
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };

    let context = crate::analysis::context_for(index, Some(path));
    let matches = frappe_syntax::extract(language, &tree, source, context);
    let absence = !is_test_file(index, path);

    let mut findings = Vec::new();
    for matched in &matches {
        for resolved in frappe_index::resolve::resolve(index, matched) {
            findings.extend(
                finding_for(&resolved)
                    .filter(|finding| {
                        absence || !matches!(finding.code, "unknown-doctype" | "unknown-field")
                    })
                    // Underline the name, not the quotes around it — the same
                    // range hover and definition use
                    // ([22](../../../docs/specs/22-hover.md)), and the text a
                    // quick fix replaces.
                    .map(|mut finding| {
                        finding.span = interior(finding.span, source);
                        finding
                    }),
            );
        }
        findings.extend(translations(matched, source));
    }
    findings
}

/// Whether absence checks should stay quiet here.
///
/// A test creates its DocTypes at runtime and sets attributes the schema never
/// declares — `frappe`'s own suite does it 134 times in a 300-file sample, and
/// every one of those is a diagnostic about code that works. Absence is
/// unknowable in a file whose whole job is to invent the thing being asked
/// about ([23](../../../docs/specs/23-diagnostics.md)).
///
/// Judged **inside the app**, never on the absolute path: a bench can live
/// anywhere, including under a directory called `tests`, and a rule that reads
/// the prefix would silence a whole workspace for where its owner happened to
/// clone it.
fn is_test_file(index: &Index, path: &Path) -> bool {
    let relative = index
        .apps()
        .iter()
        .find_map(|app| path.strip_prefix(&app.package).ok())
        .unwrap_or_else(|| Path::new(path.file_name().unwrap_or_default()));

    relative
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("test_") || name == "conftest.py")
        || relative
            .components()
            .any(|component| component.as_os_str() == "tests")
}

/// A string literal's span without its quotes.
fn interior(span: frappe_syntax::Span, source: &str) -> frappe_syntax::Span {
    let text = source.get(span.range()).unwrap_or_default();
    let Some(first) = text.chars().next() else {
        return span;
    };
    if first != '"' && first != '\'' {
        return span;
    }
    let closes = text.len() >= 2 && text.ends_with(first);
    frappe_syntax::Span::new(span.start + 1, if closes { span.end - 1 } else { span.end })
}

/// One resolved position, as a finding.
fn finding_for(resolved: &frappe_index::resolve::Resolved) -> Option<Finding> {
    use frappe_index::resolve::{FormEventKind, Target};

    Some(match &resolved.target {
        Target::UnknownDocType { name, similar } => Finding {
            code: "unknown-doctype",
            span: resolved.span,
            message: match similar.first() {
                Some(near) => format!("no DocType named `{name}`; did you mean `{near}`?"),
                None => format!("no DocType named `{name}`"),
            },
        },
        Target::UnknownField { doctype, fieldname } => Finding {
            code: "unknown-field",
            span: resolved.span,
            message: format!("`{doctype}` has no field `{fieldname}`"),
        },
        Target::FormEvent {
            name,
            doctype,
            kind: FormEventKind::Unknown,
        } => Finding {
            code: "unknown-form-event",
            span: resolved.span,
            message: format!(
                "`{name}` is neither an event nor a field on `{doctype}`; this handler never fires"
            ),
        },
        // The flagship check: a resolvable method without the decorator is a
        // guaranteed runtime PermissionError (spec 11).
        Target::Path(resolution)
            if resolution.outcome == frappe_index::paths::Outcome::WrongKind =>
        {
            Finding {
                code: "non-whitelisted-call",
                span: resolved.span,
                message: format!(
                    "`{}` is not decorated with @frappe.whitelist(); calling it raises PermissionError",
                    resolution.dotted
                ),
            }
        }
        _ => return None,
    })
}

/// `_()` and `__()` must receive a literal, because strings are extracted
/// statically — spec 23.
fn translations(matched: &frappe_syntax::Match, source: &str) -> Vec<Finding> {
    use frappe_syntax::{Role, TranslationArg};
    let _ = source;

    matched
        .slots
        .iter()
        .filter_map(|slot| {
            let Role::Translation(argument) = slot.role else {
                return None;
            };
            let what = match argument {
                TranslationArg::Interpolated => "an f-string or template literal",
                TranslationArg::Formatted => "`.format()` applied inside the call",
                TranslationArg::Concatenated => "a concatenation",
                // Spec 23 ships `translation-dynamic` off: it fires 721 times
                // on frappe itself.
                TranslationArg::Literal | TranslationArg::Dynamic => return None,
            };
            Some(Finding {
                code: "translation-literal",
                span: slot.span,
                message: format!(
                    "translations are extracted statically, so {what} is never translated"
                ),
            })
        })
        .collect()
}

/// Merge conflicts the index recorded, reported on whichever of their files is
/// open — spec 23's schema set.
fn conflicts(index: &Index, path: &Path) -> Vec<Finding> {
    index
        .conflicts()
        .iter()
        .filter(|conflict| conflict.paths.iter().any(|other| other == path))
        .map(|conflict| Finding {
            code: conflict.code.as_str(),
            // The range belongs to the last path; on the other side of a
            // duplicate the file itself is the subject.
            span: conflict
                .range
                .filter(|_| conflict.paths.last().is_some_and(|last| last == path))
                .map_or(frappe_syntax::Span::new(0, 0), |range| {
                    frappe_syntax::Span::new(range.start as usize, range.end as usize)
                }),
            message: match &conflict.fieldname {
                Some(fieldname) => format!(
                    "custom field `{fieldname}` collides with a schema field on `{}`",
                    conflict.doctype
                ),
                None => format!(
                    "`{}` is defined by more than one installed app; the later one wins",
                    conflict.doctype
                ),
            },
        })
        .collect()
}

/// Checks over a `patches.txt` — spec 23's patch set.
///
/// A patch that does not resolve never runs, and a migration that silently
/// does nothing is the failure this catches: `bench migrate` reports success
/// either way.
fn patches(index: &Index, source: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    for patch in frappe_index::hooks::read_patches(source) {
        let span = frappe_syntax::Span::new(patch.span.0, patch.span.1);

        if seen.contains(&patch.dotted) {
            findings.push(Finding {
                code: "duplicate-patch",
                span,
                message: format!("`{}` is listed more than once", patch.dotted),
            });
        }

        let resolution = index.resolve(&patch.dotted, frappe_index::paths::Kind::Module);
        match resolution.best() {
            None => findings.push(Finding {
                code: "unresolvable-patch",
                span,
                message: format!("`{}` is not a module in this workspace", patch.dotted),
            }),
            Some(candidate) => {
                // Frappe calls `execute()` and nothing else; a module without
                // one is loaded and then does nothing.
                let has_execute = index
                    .module(&candidate.path)
                    .is_some_and(|module| module.function("execute").is_some());
                if !has_execute {
                    findings.push(Finding {
                        code: "patch-without-execute",
                        span,
                        message: format!("`{}` has no execute(), so it does nothing", patch.dotted),
                    });
                }
            }
        }

        seen.push(patch.dotted);
    }

    findings
}

/// Checks over a DocType JSON — spec 23's schema set.
///
/// Every range comes from the index, which read the file from disk. A dirty
/// buffer can therefore drift from them; the alternative is re-parsing the JSON
/// on every keystroke to place a diagnostic on a schema the index has not
/// accepted yet, which would report on a document that does not exist.
fn schema(index: &Index, path: &Path) -> Vec<Finding> {
    let Some(doctype) = index.doctype_at(path) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    let mut seen: Vec<&str> = Vec::new();

    for field in &doctype.fields {
        let fieldname = index.fieldname(field);
        let fieldtype = index.fieldtype(field);

        // A field declared twice: the later one wins silently, and a form that
        // reads the first never sees the value the second holds.
        if seen.contains(&fieldname) {
            findings.push(Finding {
                code: "duplicate-fieldname",
                span: span_of(field.ranges.fieldname.unwrap_or(field.range)),
                message: format!("`{fieldname}` is declared more than once"),
            });
        } else {
            seen.push(fieldname);
        }

        let options_span = span_of(field.ranges.options.unwrap_or(field.range));
        match index.options(field) {
            Options::LinkTarget(target) => {
                if index.doctype(target).is_none() {
                    findings.push(Finding {
                        code: "unknown-link-target",
                        span: options_span,
                        message: format!(
                            "`{fieldname}` links to `{target}`, which is not a DocType"
                        ),
                    });
                }
            }
            Options::ChildDocType(child) => match index.doctype(child) {
                None => findings.push(Finding {
                    code: "unknown-link-target",
                    span: options_span,
                    message: format!("`{fieldname}` holds `{child}`, which is not a DocType"),
                }),
                // A Table of a non-child DocType does not load: `istable` is
                // what makes a DocType usable as rows.
                Some(target) if !target.is_child => findings.push(Finding {
                    code: "table-not-child",
                    span: options_span,
                    message: format!(
                        "`{child}` is not a child table, so `{fieldname}` cannot hold it"
                    ),
                }),
                Some(_) => {}
            },
            Options::DynamicSource(sibling)
                if index.dynamic_link_source(doctype, field).is_none() =>
            {
                findings.push(Finding {
                    code: "dangling-dynamic-link",
                    span: options_span,
                    message: format!(
                        "`{fieldname}` takes its target from `{sibling}`, which is not a field here"
                    ),
                });
            }
            _ => {}
        }

        if let Some(value) = &field.fetch_from
            && let Some(message) = unresolvable_fetch(index, doctype, field, value)
        {
            findings.push(Finding {
                code: "unresolvable-fetch-from",
                span: span_of(field.ranges.fetch_from.unwrap_or(field.range)),
                message,
            });
        }

        if let Some(expression) = &field.depends_on {
            for referenced in referenced_fields(expression) {
                // `__islocal`, `__unsaved`, `__onload`: flags the client puts on
                // the form's document object, never columns. frappe's own
                // schema reads them 22 times.
                if !referenced.starts_with("__") && !index.has_field(&doctype.name, referenced) {
                    findings.push(Finding {
                        code: "dangling-depends-on",
                        span: span_of(field.ranges.depends_on.unwrap_or(field.range)),
                        message: format!("`{referenced}` is not a field on `{}`", doctype.name),
                    });
                }
            }
        }

        let _ = fieldtype;
    }

    // `field_order` decides what the form shows; a field missing from it is
    // invisible, and a name in it that no field has renders nothing.
    let declared: Vec<&str> = doctype.fields.iter().map(|f| index.fieldname(f)).collect();
    let ordered: Vec<&str> = doctype
        .field_order
        .iter()
        .map(|symbol| index.text(*symbol))
        .collect();
    if !ordered.is_empty() {
        // Only the direction that is provably broken. A field absent from
        // `field_order` is ordinary — Frappe regenerates the list on save, and
        // its own schema ships two such fields — but a name in `field_order`
        // that no field has renders nothing at all.
        for extra in ordered.iter().filter(|name| !declared.contains(name)) {
            findings.push(Finding {
                code: "field-order-mismatch",
                span: frappe_syntax::Span::new(0, 0),
                message: format!("field_order names `{extra}`, which is not a field"),
            });
        }
    }

    for (property, value) in [
        ("title_field", &doctype.title_field),
        ("sort_field", &doctype.sort_field),
        ("image_field", &doctype.image_field),
    ] {
        if let Some(value) = value
            && !value.is_empty()
            && !index.has_field(&doctype.name, value)
        {
            findings.push(Finding {
                code: "unknown-meta-field",
                span: frappe_syntax::Span::new(0, 0),
                message: format!("{property} names `{value}`, which is not a field"),
            });
        }
    }

    findings
}

/// Why a `fetch_from` cannot work, if it cannot.
///
/// `<link>.<target>`: the link half must be a field here, and the target half
/// must be a field on what it links to — counting implicit fields, since
/// `owner` and `modified` are fetched constantly. A `Dynamic Link` source names
/// its DocType at runtime, so nothing here can say whether the target exists.
fn unresolvable_fetch(
    index: &Index,
    doctype: &frappe_index::DocType,
    field: &frappe_index::Field,
    value: &str,
) -> Option<String> {
    let split = frappe_index::doctype::split_fetch_from(value)?;
    let (link, _) = index.fetch_from(doctype, field);

    let Some(link_field) = link else {
        return Some(format!(
            "`{value}` reads `{}`, which is not a field here",
            split.link_fieldname
        ));
    };

    match index.options(link_field) {
        Options::LinkTarget(target) => {
            // An unknown link target is already reported against the link
            // field itself; saying it twice helps nobody.
            let known = index.doctype(target).is_some();
            (known && !index.has_field(target, split.target_fieldname)).then(|| {
                format!(
                    "`{target}` has no field `{}`, so `{}` stays empty",
                    split.target_fieldname,
                    index.fieldname(field)
                )
            })
        }
        // Runtime-determined target: unknowable, so silent.
        _ => None,
    }
}

/// The sibling fieldnames a `depends_on` expression reads.
///
/// `eval:` expressions name them as `doc.<field>`; anything else is a bare
/// fieldname. Only the forms Frappe itself evaluates are read — an expression
/// this cannot follow yields nothing rather than a guess.
fn referenced_fields(expression: &str) -> Vec<&str> {
    let expression = expression.trim();
    let Some(rest) = expression.strip_prefix("eval:") else {
        return if expression
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !expression.is_empty()
        {
            vec![expression]
        } else {
            Vec::new()
        };
    };

    let mut found = Vec::new();
    let mut remaining = rest;
    while let Some(at) = remaining.find("doc.") {
        let after = &remaining[at + 4..];
        let end = after
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(after.len());
        if end > 0 {
            found.push(&after[..end]);
        }
        remaining = &after[end..];
    }
    found
}

fn span_of(range: frappe_index::Range) -> frappe_syntax::Span {
    frappe_syntax::Span::new(range.start as usize, range.end as usize)
}

/// A DocType JSON that does not parse — spec 02's failure policy, and the one
/// diagnostic that needs no judgement at all.
fn malformed(index: &Index, path: &Path) -> Vec<Finding> {
    index
        .problems()
        .iter()
        .filter(|problem| problem.path == path)
        .filter(|problem| problem.message.starts_with("malformed JSON"))
        .map(|problem| Finding {
            code: "malformed-json",
            // The whole file: a parse failure has no meaningful position, and
            // the parser's own offset points at the symptom, not the cause.
            span: frappe_syntax::Span::new(0, 0),
            message: problem.message.clone(),
        })
        .collect()
}

/// Checks that assert something is *not* there, which only an index complete
/// enough to know may make — spec 23's confidence table.
const ABSENCE_CHECKS: [&str; 12] = [
    "dangling-depends-on",
    "dangling-dynamic-link",
    "missing-asset",
    "non-whitelisted-call",
    "table-not-child",
    "unknown-doctype",
    "unknown-field",
    "unknown-form-event",
    "unknown-link-target",
    "unknown-meta-field",
    "unknown-required-app",
    "unresolvable-fetch-from",
];

fn asserts_absence(code: &str) -> bool {
    ABSENCE_CHECKS.binary_search(&code).is_ok()
}

/// Whether the index knows enough to say something is missing.
///
/// `Partial` is the first 150 ms of every session, and `Degraded` is a bridge
/// that was configured and could not be reached. In both, a name the index
/// cannot find may simply not have been read yet — reporting it would mean the
/// server greets the user with errors about their own working code, which is
/// exactly how a diagnostic set gets switched off
/// ([23](../../../docs/specs/23-diagnostics.md)).
fn absence_is_knowable(index: &Index) -> bool {
    matches!(index.confidence(), Confidence::Full | Confidence::Static)
}

/// Whether checks decided by a vendored table may fire.
///
/// Spec 31: unknown never becomes invalid unless the detected version matches
/// a vendored snapshot exactly. On a fork, an unreleased branch, or a commit
/// between snapshots, an unrecognized value degrades to "unknown" and reports
/// nothing. This is what branch-agnostic costs, and it is the correct trade: a
/// server that calls a valid new hook key an error on the day it lands upstream
/// is worse than one that stays quiet.
fn version_matches(index: &Index) -> bool {
    index
        .workspace()
        .frappe_version()
        .is_some_and(|version| version == frappe_index::index::vendored_snapshot())
}

/// The configured severity, or `None` when the check is off — spec 30.
fn severity(config: &Config, code: &str) -> Option<DiagnosticSeverity> {
    let level = match code {
        "unknown-hook" => config.diagnostics.unknown_hook,
        "unknown-doc-event" => config.diagnostics.unknown_doc_event,
        "unknown-doctype" => config.diagnostics.unknown_doctype,
        "unknown-field" => config.diagnostics.unknown_field,
        "non-whitelisted-call" => config.diagnostics.non_whitelisted_call,
        "translation-literal" => config.diagnostics.translation_literal,
        // Codes spec 30 does not name individually take the default from
        // spec 23's table.
        "malformed-json" | "unknown-frequency" | "invalid-cron" => Level::Error,
        "unknown-form-event" => config.diagnostics.unknown_field,
        _ => Level::Warning,
    };

    match level {
        Level::Error => Some(DiagnosticSeverity::ERROR),
        Level::Warning => Some(DiagnosticSeverity::WARNING),
        Level::Info => Some(DiagnosticSeverity::INFORMATION),
        Level::Hint => Some(DiagnosticSeverity::HINT),
        Level::Off => None,
    }
}

/// `# frappe-lsp: ignore` on the line or the line above — spec 23.
fn suppressed(source: &str, lines: &LineIndex<'_>, finding: &Finding) -> bool {
    let line = lines.position(finding.span.start).line as usize;
    let text: Vec<&str> = source.lines().collect();

    [line.checked_sub(1), Some(line)]
        .into_iter()
        .flatten()
        .filter_map(|number| text.get(number))
        .any(|candidate| ignores(candidate, finding.code))
}

fn ignores(line: &str, code: &str) -> bool {
    let Some(rest) = line.split("frappe-lsp:").nth(1) else {
        return false;
    };
    let rest = rest.trim();
    let Some(codes) = rest.strip_prefix("ignore") else {
        return false;
    };
    let codes = codes.trim();
    // Bare `ignore` silences the line; `ignore <code>` silences one check.
    codes.is_empty() || codes.split_whitespace().any(|named| named == code)
}

/// Diagnostics for a position range, as the pull transport wants them.
#[must_use]
pub fn empty_range() -> Range {
    Range {
        start: Position::new(0, 0),
        end: Position::new(0, 0),
    }
}

/// Quick fixes for our own diagnostics — [23](../../../docs/specs/23-diagnostics.md).
///
/// Only where the fix is unambiguous: a single candidate, or a decorator whose
/// absence is the whole defect. There is deliberately no "fix all" — a wrong
/// bulk edit across a bench is unrecoverable.
#[must_use]
pub fn fixes(
    index: &Index,
    uri: &Uri,
    source: &str,
    diagnostics: &[Diagnostic],
) -> Vec<tower_lsp_server::ls_types::CodeActionOrCommand> {
    use tower_lsp_server::ls_types::{
        CodeAction, CodeActionKind, CodeActionOrCommand, TextEdit, WorkspaceEdit,
    };

    let lines = LineIndex::new(source, frappe_syntax::PositionEncoding::Utf16);
    let mut actions = Vec::new();

    for diagnostic in diagnostics {
        let Some(NumberOrString::String(code)) = &diagnostic.code else {
            continue;
        };
        let Some(code) = code.strip_prefix("frappe-lsp/") else {
            continue;
        };
        // The text the diagnostic covers, which is what a replacement replaces.
        let written = slice(source, &lines, diagnostic.range);

        let replacement = match code {
            "unknown-doctype" => single(index.case_insensitive_matches(&written)),
            "unknown-hook" => near_miss(&written).map(str::to_string),
            "unknown-field" => {
                let doctype = diagnostic
                    .message
                    .split('`')
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                single(
                    index
                        .field_names(&doctype)
                        .into_iter()
                        .filter(|name| name.eq_ignore_ascii_case(&written))
                        .collect(),
                )
            }
            _ => None,
        };

        let Some(replacement) = replacement else {
            continue;
        };
        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("Replace with `{replacement}`"),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![diagnostic.clone()]),
            edit: Some(WorkspaceEdit {
                changes: Some(
                    [(
                        uri.clone(),
                        vec![TextEdit {
                            range: diagnostic.range,
                            new_text: replacement,
                        }],
                    )]
                    .into_iter()
                    .collect(),
                ),
                ..Default::default()
            }),
            is_preferred: Some(true),
            ..Default::default()
        }));
    }

    actions
}

/// A candidate list is only a fix when it holds exactly one — spec 23 allows a
/// replacement for a single candidate and nothing more.
fn single(candidates: Vec<&str>) -> Option<String> {
    match candidates.as_slice() {
        [only] => Some((*only).to_string()),
        _ => None,
    }
}

/// The source text a range covers.
fn slice(source: &str, lines: &LineIndex<'_>, range: Range) -> String {
    let offset = |position: Position| -> usize {
        source
            .split_inclusive('\n')
            .take(position.line as usize)
            .map(str::len)
            .sum::<usize>()
            + position.character as usize
    };
    let _ = lines;
    let (start, end) = (offset(range.start), offset(range.end));
    source.get(start..end).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_ignore_silences_the_line_and_a_named_one_is_specific() {
        assert!(ignores("x = 1  # frappe-lsp: ignore", "unknown-hook"));
        assert!(ignores(
            "x = 1  # frappe-lsp: ignore unknown-hook",
            "unknown-hook"
        ));
        assert!(!ignores(
            "x = 1  # frappe-lsp: ignore invalid-cron",
            "unknown-hook"
        ));
        assert!(!ignores("x = 1  # a normal comment", "unknown-hook"));
    }

    #[test]
    fn an_off_check_produces_nothing() {
        let mut config = Config::default();
        assert!(severity(&config, "unknown-hook").is_some());

        config.diagnostics.unknown_hook = Level::Off;
        assert!(severity(&config, "unknown-hook").is_none());
    }

    #[test]
    fn the_absence_list_is_sorted_and_holds_only_absence_checks() {
        assert!(ABSENCE_CHECKS.windows(2).all(|pair| pair[0] < pair[1]));

        // A wrong event name or an invalid cron expression is wrong however
        // complete the index is, so neither is ever gated (spec 23).
        for presence in [
            "unknown-doc-event",
            "invalid-cron",
            "unknown-frequency",
            "unknown-hook",
            "duplicate-hook",
            "malformed-json",
            "translation-literal",
        ] {
            assert!(!asserts_absence(presence), "{presence}");
        }
    }

    #[test]
    fn a_partial_index_says_nothing_about_absence() {
        // The first 150 ms of every session. A name it cannot find may simply
        // not have been read yet.
        let partial = Index::partial();
        assert!(!absence_is_knowable(&partial));

        let found = for_buffer(
            &partial,
            &Config::default(),
            Path::new("/bench/apps/x/x/api.py"),
            "import frappe\nfrappe.ui.form.on(\"Task\", { nosuchthing(frm) {} })\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_bench_under_a_tests_directory_is_not_all_tests() {
        // The fixture bench lives at `<repo>/tests/fixtures/bench`, and reading
        // the absolute path silenced every absence check in it.
        let index = Index::build(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .expect("repo root")
                .join("tests/fixtures/bench"),
        );
        let app = index
            .apps()
            .iter()
            .find(|app| app.name == "library")
            .expect("library");

        assert!(!is_test_file(&index, &app.package.join("api.py")));
        // A real test inside the app still counts as one.
        assert!(is_test_file(
            &index,
            &app.package
                .join("library/doctype/library_member/test_library_member.py")
        ));
        assert!(is_test_file(&index, &app.package.join("tests/helpers.py")));
    }

    #[test]
    fn python_conventions_keep_helpers_out_of_the_check() {
        // A hooks.py holds helpers, and accusing them would make the check
        // unusable on real apps.
        assert!(is_private_name("_QUERY_CONDITIONS"));
        assert!(is_private_name("MY_CONSTANT"));
        assert!(!is_private_name("doctype_javascript"));
        assert!(!is_private_name("app_include_js"));
    }

    #[test]
    fn a_near_miss_carries_its_own_fix() {
        assert_eq!(near_miss("doctype_j"), Some("doctype_js"));
        assert_eq!(near_miss("after_instal"), Some("after_install"));
        // No suggestion is offered where none is credible.
        assert_eq!(near_miss("doctype_javascript"), None);
    }

    #[test]
    fn the_codes_carry_the_documented_prefix() {
        // Spec 23: without a code, suppression is unusable.
        assert_eq!(format!("{PREFIX}/unknown-hook"), "frappe-lsp/unknown-hook");
    }
}
