//! Running a Frappe test from the file it lives in —
//! [25](../../../docs/specs/25-test-runner.md).
//!
//! The one feature that executes anything. Everything else in this server
//! reads files, so the rules are strict: discovery never depends on execution
//! being possible, a read-only checkout still shows what is runnable and why
//! it is not, and nothing here touches the index or the surfaces.

use std::path::Path;

use frappe_index::Index;
use frappe_syntax::symbols::{Class, Module};
use tower_lsp_server::ls_types::{CodeLens, Command, Range};

use crate::config::Config;
use crate::locations::LineIndex;

/// The base classes frappe's own test classes derive from.
///
/// Matched by name, plus one level of local subclassing inside the same file.
/// Walking a cross-module hierarchy would need the whole class graph to answer
/// a question a code lens asks on every keystroke
/// ([25](../../../docs/specs/25-test-runner.md)).
const BASES: [&str; 4] = [
    "IntegrationTestCase",
    "MockedRequestTestCase",
    "TestCase",
    "UnitTestCase",
];

/// What a lens carries, and the only thing `workspace/executeCommand` trusts.
///
/// Not an argv. A lens travels through the client, so what comes back is a
/// request rather than evidence; the server rebuilds the command from this
/// against the index it holds ([25](../../../docs/specs/25-test-runner.md)).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invocation {
    /// The app the module belongs to.
    pub app: String,
    /// The dotted module path, or the DocType name when `doctype` is set.
    #[serde(default)]
    pub module: String,
    /// A DocType scope, which takes `--doctype` instead of `--module`.
    #[serde(default)]
    pub doctype: Option<String>,
    #[serde(default)]
    pub case: Option<String>,
    #[serde(default)]
    pub test: Option<String>,
}

/// A runnable scope in one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// The class, or `None` for the module itself.
    pub case: Option<String>,
    /// The `test_*` method, or `None` for the whole class.
    pub test: Option<String>,
    /// Where the lens goes.
    pub span: frappe_syntax::Span,
}

/// Whether the file is a test module at all.
#[must_use]
pub fn is_test_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("test_") && name.ends_with(".py"))
}

/// Every runnable scope: the module, each test class, each `test_*` method.
///
/// `index` supplies the class graph, which reaches bases in other modules —
/// `ERPNextTestSuite` and the mixins under it. Without it the check is the
/// purely local one, which is all a file on its own can support.
#[must_use]
pub fn scopes(index: Option<&Index>, module: &Module) -> Vec<Scope> {
    let cases: Vec<&Class> = module
        .classes
        .iter()
        .filter(|class| is_case(index, class, &module.classes))
        .collect();
    if cases.is_empty() {
        return Vec::new();
    }

    // The module lens sits on the first line, which is where a reader looks
    // for a file-wide action.
    let mut scopes = vec![Scope {
        case: None,
        test: None,
        span: frappe_syntax::Span::new(0, 0),
    }];

    for class in cases {
        scopes.push(Scope {
            case: Some(class.name.clone()),
            test: None,
            span: class.name_span,
        });
        for method in &class.methods {
            if method.name.starts_with("test_") {
                scopes.push(Scope {
                    case: Some(class.name.clone()),
                    test: Some(method.name.clone()),
                    span: method.name_span,
                });
            }
        }
    }
    scopes
}

/// Whether a class is a test case: a direct base-name match, or one level of
/// subclassing a class in the same file that is.
fn is_case(index: Option<&Index>, class: &Class, siblings: &[Class]) -> bool {
    let names = |class: &Class| -> Vec<String> {
        class
            .bases
            .iter()
            .map(|base| base.rsplit('.').next().unwrap_or(base).to_string())
            .collect()
    };

    let direct = names(class);
    if direct.iter().any(|base| BASES.contains(&base.as_str())) {
        return true;
    }
    // The workspace's class graph, when there is one: erpnext's tests derive
    // from `ERPNextTestSuite` in another module, and 484 of them would
    // otherwise show no lens at all.
    if let Some(index) = index
        && direct.iter().any(|base| index.derives_from(base, &BASES))
    {
        return true;
    }
    direct.iter().any(|base| {
        siblings
            .iter()
            .filter(|sibling| sibling.name == *base)
            .any(|sibling| names(sibling).iter().any(|b| BASES.contains(&b.as_str())))
    })
}

/// Rebuild the command for an invocation, validating it against the index.
///
/// `None` when the app is not in this workspace or no site resolves — the two
/// ways a lens can outlive what produced it.
#[must_use]
pub fn rebuild(
    index: &Index,
    config: &Config,
    chosen: Option<&str>,
    invocation: &Invocation,
) -> Option<Vec<String>> {
    if !index.apps().iter().any(|app| app.name == invocation.app) {
        return None;
    }
    let SiteChoice::Resolved(site) = site(index, config, chosen) else {
        return None;
    };

    if let Some(doctype) = &invocation.doctype {
        index.doctype(doctype)?;
        return Some(doctype_argv(config, &site, &invocation.app, doctype));
    }
    let scope = Scope {
        case: invocation.case.clone(),
        test: invocation.test.clone(),
        span: frappe_syntax::Span::new(0, 0),
    };
    Some(argv(
        config,
        &site,
        &invocation.app,
        &invocation.module,
        &scope,
    ))
}

/// Which site a run would use, and why it might be none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiteChoice {
    /// The site to pass as `--site`.
    Resolved(String),
    /// Several sites and nothing configured. Execution is disabled rather than
    /// guessed: guessing runs a suite against the wrong database.
    Ambiguous(Vec<String>),
    /// No site at all — a read-only checkout, which is a supported state.
    None,
}

impl SiteChoice {
    /// Why the lens is not runnable, for the title.
    #[must_use]
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Resolved(_) => None,
            // Ambiguity is answerable, so the title invites the answer rather
            // than reporting a dead end.
            Self::Ambiguous(_) => Some("choose a site"),
            Self::None => Some("no site"),
        }
    }
}

/// `currentsite.txt`, else the sole site, else configuration — spec 25.
///
/// `chosen` is what the user picked this session, which beats ambiguity but
/// not an explicit `test.site`: configuration is a decision already made.
#[must_use]
pub fn site(index: &Index, config: &Config, chosen: Option<&str>) -> SiteChoice {
    if !config.test.site.is_empty() {
        return SiteChoice::Resolved(config.test.site.clone());
    }
    if let Some(chosen) = chosen.filter(|name| !name.is_empty()) {
        return SiteChoice::Resolved(chosen.to_string());
    }

    let workspace = index.workspace();
    if let Some(current) = workspace.current_site.as_deref()
        && workspace.sites.iter().any(|site| site.name == current)
    {
        return SiteChoice::Resolved(current.to_string());
    }
    match workspace.sites.as_slice() {
        [] => SiteChoice::None,
        [only] => SiteChoice::Resolved(only.name.clone()),
        several => SiteChoice::Ambiguous(several.iter().map(|s| s.name.clone()).collect()),
    }
}

/// The `bench run-tests` invocation for one scope.
#[must_use]
pub fn argv(config: &Config, site: &str, app: &str, dotted: &str, scope: &Scope) -> Vec<String> {
    let mut argv = vec![
        config.test.bench.clone(),
        "--site".to_string(),
        site.to_string(),
        "run-tests".to_string(),
        "--app".to_string(),
        app.to_string(),
        "--module".to_string(),
        dotted.to_string(),
    ];
    if let Some(case) = &scope.case {
        argv.push("--case".to_string());
        argv.push(case.clone());
    }
    if let Some(test) = &scope.test {
        argv.push("--test".to_string());
        argv.push(test.clone());
    }
    // Flags are configuration rather than separate lenses: one lens per scope,
    // and a lens menu is noise ([25](../../../docs/specs/25-test-runner.md)).
    for (enabled, flag) in [
        (config.test.failfast, "--failfast"),
        (config.test.profile, "--profile"),
        (config.test.coverage, "--coverage"),
        (config.test.lightmode, "--lightmode"),
    ] {
        if enabled {
            argv.push(flag.to_string());
        }
    }
    argv
}

/// The `--doctype` invocation for a DocType JSON whose `test_<x>.py` exists.
#[must_use]
pub fn doctype_argv(config: &Config, site: &str, app: &str, doctype: &str) -> Vec<String> {
    let mut argv = vec![
        config.test.bench.clone(),
        "--site".to_string(),
        site.to_string(),
        "run-tests".to_string(),
        "--app".to_string(),
        app.to_string(),
        "--doctype".to_string(),
        doctype.to_string(),
    ];
    for (enabled, flag) in [
        (config.test.failfast, "--failfast"),
        (config.test.profile, "--profile"),
        (config.test.coverage, "--coverage"),
        (config.test.lightmode, "--lightmode"),
    ] {
        if enabled {
            argv.push(flag.to_string());
        }
    }
    argv
}

/// The lenses for one file.
///
/// A file with no site still gets its lenses, titled with the reason it cannot
/// run and carrying no command. Discovery never depends on execution being
/// possible ([25](../../../docs/specs/25-test-runner.md)).
#[must_use]
pub fn lenses(
    index: &Index,
    config: &Config,
    chosen: Option<&str>,
    path: &Path,
    source: &str,
) -> Vec<CodeLens> {
    if !config.test.enabled {
        return Vec::new();
    }

    let choice = site(index, config, chosen);
    let lines = LineIndex::new(source, frappe_syntax::PositionEncoding::Utf16);
    let mut lenses = Vec::new();

    if let Some(doctype) = doctype_lens_target(index, path) {
        let Some((app, _)) = index.dotted_module(&doctype.path.with_extension("py")) else {
            return lenses;
        };
        lenses.push(CodeLens {
            range: Range {
                start: lines.position(0),
                end: lines.position(0),
            },
            command: Some(command(
                decorate("Run DocType tests", &choice),
                &choice,
                &Invocation {
                    app: app.to_string(),
                    module: String::new(),
                    doctype: Some(doctype.name.clone()),
                    case: None,
                    test: None,
                },
            )),
            data: None,
        });
        return lenses;
    }

    if !is_test_file(path) {
        return Vec::new();
    }
    let Ok(module) = frappe_syntax::symbols::read(source) else {
        return Vec::new();
    };
    let Some((app, dotted)) = index.dotted_module(path) else {
        return Vec::new();
    };

    for scope in scopes(Some(index), &module) {
        let title = decorate(
            match (&scope.case, &scope.test) {
                (None, _) => "Run module",
                (Some(_), None) => "Run class",
                (Some(_), Some(_)) => "Run test",
            },
            &choice,
        );
        let range = Range {
            start: lines.position(scope.span.start),
            end: lines.position(scope.span.end),
        };
        lenses.push(CodeLens {
            range,
            command: Some(command(
                title,
                &choice,
                &Invocation {
                    app: app.to_string(),
                    module: dotted.clone(),
                    doctype: None,
                    case: scope.case.clone(),
                    test: scope.test.clone(),
                },
            )),
            data: None,
        });
    }
    lenses
}

/// The DocType a JSON file defines, when a `test_<scrubbed>.py` sits beside it.
fn doctype_lens_target<'a>(index: &'a Index, path: &Path) -> Option<&'a frappe_index::DocType> {
    if path.extension().is_none_or(|extension| extension != "json") {
        return None;
    }
    let doctype = index.doctype_at(path)?;
    let stem = path.file_stem()?.to_str()?;
    path.with_file_name(format!("test_{stem}.py"))
        .is_file()
        .then_some(doctype)
}

fn decorate(title: &str, choice: &SiteChoice) -> String {
    match choice.reason() {
        Some(reason) => format!("{title} — {reason}"),
        None => title.to_string(),
    }
}

/// The command a lens carries.
///
/// With a site resolved this is `frappe-lsp.runTest`, which the client spawns
/// in a terminal — the user gets a TTY, colour and their own scrollback, and
/// the server never sees the output.
///
/// With several sites and none configured it is `frappe-lsp.selectSite`, which
/// asks. Asking is not guessing: spec 25 forbids the second because it runs a
/// suite against the wrong database, and the first is how the user says which
/// one they meant. With no site at all there is nothing to ask, so the lens is
/// inert and says why.
fn command(title: String, choice: &SiteChoice, invocation: &Invocation) -> Command {
    match choice {
        SiteChoice::Resolved(_) => Command {
            title,
            command: crate::capabilities::RUN_TEST.to_string(),
            arguments: Some(vec![serde_json::json!(invocation)]),
        },
        SiteChoice::Ambiguous(sites) => Command {
            title,
            command: crate::capabilities::SELECT_SITE.to_string(),
            arguments: Some(vec![serde_json::json!(sites)]),
        },
        SiteChoice::None => Command {
            title,
            command: String::new(),
            arguments: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(source: &str) -> Module {
        frappe_syntax::symbols::read(source).expect("the grammar")
    }

    #[test]
    fn a_test_case_is_found_by_base_name_and_one_level_of_subclassing() {
        let found = module(
            "\
from frappe.tests import IntegrationTestCase


class Base(IntegrationTestCase):
\tdef helper(self):
\t\tpass


class TestThing(Base):
\tdef test_one(self):
\t\tpass

\tdef helper(self):
\t\tpass


class NotATest:
\tdef test_two(self):
\t\tpass
",
        );

        let scopes = scopes(None, &found);
        let cases: Vec<Option<&str>> = scopes.iter().map(|scope| scope.case.as_deref()).collect();
        // The module, both cases, and the one `test_*` method on the subclass.
        assert_eq!(
            cases,
            [None, Some("Base"), Some("TestThing"), Some("TestThing")]
        );

        let tests: Vec<&str> = scopes
            .iter()
            .filter_map(|scope| scope.test.as_deref())
            .collect();
        assert_eq!(tests, ["test_one"], "only `test_*` methods are runnable");
    }

    #[test]
    fn a_file_with_no_test_case_has_no_scopes() {
        // Spec 25 resolves syntactically: a class deriving from something this
        // file cannot see is not assumed to be a test.
        let found =
            module("class TestThing(SomethingElsewhere):\n\tdef test_one(self):\n\t\tpass\n");
        assert!(scopes(None, &found).is_empty());
    }

    #[test]
    fn flags_are_configuration_rather_than_extra_lenses() {
        let mut config = Config::default();
        config.test.failfast = true;
        config.test.coverage = true;

        let scope = Scope {
            case: Some("TestThing".to_string()),
            test: Some("test_one".to_string()),
            span: frappe_syntax::Span::new(0, 0),
        };
        let argv = argv(
            &config,
            "a.localhost",
            "library",
            "library.tests.test_thing",
            &scope,
        );

        assert_eq!(
            argv,
            [
                "bench",
                "--site",
                "a.localhost",
                "run-tests",
                "--app",
                "library",
                "--module",
                "library.tests.test_thing",
                "--case",
                "TestThing",
                "--test",
                "test_one",
                "--failfast",
                "--coverage",
            ]
        );
    }

    /// The fixture bench, copied, so a test can take its sites away.
    fn bench_without_sites(name: &str) -> std::path::PathBuf {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repo root")
            .join("tests/fixtures/bench");
        let target =
            std::env::temp_dir().join(format!("frappe-lsp-testing-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&target);
        copy_tree(&source, &target);

        // A read-only checkout: apps on disk, no site to run against.
        let sites = target.join("sites");
        for entry in std::fs::read_dir(&sites).expect("sites").flatten() {
            if entry.path().is_dir() && entry.path().join("site_config.json").is_file() {
                std::fs::remove_dir_all(entry.path()).expect("remove site");
            }
        }
        let _ = std::fs::remove_file(sites.join("currentsite.txt"));
        target
    }

    fn copy_tree(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("create");
        for entry in std::fs::read_dir(from).expect("readable").flatten() {
            let source = entry.path();
            let target = to.join(entry.file_name());
            if source.is_dir() {
                copy_tree(&source, &target);
            } else {
                std::fs::copy(&source, &target).expect("copy");
            }
        }
    }

    #[test]
    fn a_checkout_with_no_site_still_shows_what_is_runnable() {
        // Spec 25: discovery MUST NOT depend on execution being possible.
        let bench = bench_without_sites("nosite");
        let index = Index::build(&bench);
        let config = Config::default();
        assert_eq!(site(&index, &config, None), SiteChoice::None);

        let path = bench
            .join("apps/library/library/library/doctype/library_member/test_library_member.py");
        let source = std::fs::read_to_string(&path).expect("readable");
        let lenses = lenses(&index, &config, None, &path, &source);

        assert_eq!(lenses.len(), 3, "the lenses are discovery, not execution");
        for lens in &lenses {
            let command = lens.command.as_ref().expect("a command");
            assert!(command.title.ends_with("— no site"), "{}", command.title);
            // Inert: nothing to run, so nothing is offered to run.
            assert!(command.command.is_empty(), "{command:?}");
            assert!(command.arguments.is_none());
        }

        std::fs::remove_dir_all(&bench).expect("cleanup");
    }

    #[test]
    fn ambiguity_asks_rather_than_guessing() {
        // Guessing here runs a suite against the wrong database (specs/25).
        let ambiguous = SiteChoice::Ambiguous(vec!["a".into(), "b".into()]);
        assert_eq!(ambiguous.reason(), Some("choose a site"));
        assert_eq!(SiteChoice::None.reason(), Some("no site"));
        assert_eq!(SiteChoice::Resolved("a".into()).reason(), None);

        // A chosen site resolves the ambiguity; explicit configuration still
        // wins over it, because that is a decision already made.
        let bench = bench_without_sites("chosen");
        let index = Index::build(&bench);
        let mut config = Config::default();
        assert_eq!(
            site(&index, &config, Some("b.localhost")),
            SiteChoice::Resolved("b.localhost".to_string())
        );
        config.test.site = "configured".to_string();
        assert_eq!(
            site(&index, &config, Some("b.localhost")),
            SiteChoice::Resolved("configured".to_string())
        );
        std::fs::remove_dir_all(&bench).expect("cleanup");
    }

    #[test]
    fn a_lens_carries_a_scope_rather_than_a_command_line() {
        // What comes back through the client is a request, not evidence: the
        // server rebuilds the command from the index it holds (specs/25).
        let bench = bench_without_sites("rebuild");
        let index = Index::build(&bench);
        let config = Config::default();

        let invocation = Invocation {
            app: "library".to_string(),
            module: "library.tests.test_thing".to_string(),
            doctype: None,
            case: Some("TestThing".to_string()),
            test: None,
        };
        // No site: nothing to run.
        assert!(rebuild(&index, &config, None, &invocation).is_none());
        // A site, and an app this workspace has.
        let argv = rebuild(&index, &config, Some("a.localhost"), &invocation).expect("argv");
        assert_eq!(argv[0], "bench");
        assert!(argv.contains(&"--case".to_string()));

        // An app it does not.
        let foreign = Invocation {
            app: "not_installed".to_string(),
            ..invocation
        };
        assert!(rebuild(&index, &config, Some("a.localhost"), &foreign).is_none());

        std::fs::remove_dir_all(&bench).expect("cleanup");
    }
}
