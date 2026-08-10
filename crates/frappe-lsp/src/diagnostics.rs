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

use frappe_index::Index;
use frappe_syntax::generated;
use tower_lsp_server::ls_types::{
    CodeDescription, Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range, Uri,
};

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

    let mut findings = Vec::new();
    if crate::hooks::is_hooks_file(path) {
        findings.extend(hooks(index, source, path));
    } else if let Some(language) = frappe_syntax::Language::from_path(path)
        && crate::analysis::supported(language)
    {
        findings.extend(code(index, language, path, source));
    }
    findings.extend(malformed(index, path));
    findings.extend(conflicts(index, path));

    let lines = LineIndex::new(source, frappe_syntax::PositionEncoding::Utf16);
    findings
        .into_iter()
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
                if !is_private_name(name) {
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
            findings.extend(finding_for(&resolved).filter(|finding| {
                absence || !matches!(finding.code, "unknown-doctype" | "unknown-field")
            }));
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
