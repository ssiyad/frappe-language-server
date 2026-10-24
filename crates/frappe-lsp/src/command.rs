//! The one-shot commands of [26](../../../docs/specs/26-command-line.md).
//!
//! A coding agent reads files with its own tools, so an editor's diagnostics
//! never reach it, and its LSP client cannot call the custom methods of
//! [30](../../../docs/specs/30-configuration.md). These are the same answers
//! for anything with a shell.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use frappe_index::Index;
use frappe_syntax::PositionEncoding;
use serde_json::{Value, json};
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

use crate::cli::Command;
use crate::config::{Config, ConfigSources};

/// Exit status when a command answered and found something to report.
const FOUND: u8 = 1;
/// Exit status when a command could not answer at all.
const FAILED: u8 = 2;

pub fn run(command: Command) -> ExitCode {
    let outcome = match command {
        Command::Check { paths, json } => check(&paths, json),
        Command::Describe { doctype, json } => describe(&doctype, json),
        Command::References {
            doctype,
            fieldname,
            json,
        } => references(&doctype, fieldname.as_deref(), json),
    };
    outcome.unwrap_or_else(|message| {
        eprintln!("frappe-lsp: {message}");
        ExitCode::from(FAILED)
    })
}

/// The index and configuration the server would have for this bench.
struct Bench {
    index: Index,
    config: Config,
}

impl Bench {
    /// The bench above `start`, from the server's cache when it is current.
    fn open(start: &Path) -> Result<Self, String> {
        let (sources, errors) = ConfigSources::load(&[start.to_path_buf()], None);
        for error in errors {
            eprintln!("frappe-lsp: {error}");
        }
        let root = sources
            .bench_root()
            .filter(|root| {
                frappe_index::detect::detect(root).mode != frappe_index::detect::Mode::None
            })
            .ok_or_else(|| format!("no Frappe bench or app above {}", start.display()))?
            .to_path_buf();
        let config = sources.resolve();
        let index = load_index(&root, &config);
        Ok(Self { index, config })
    }
}

/// Warm from the cache, or build and scan once and leave a cache behind, so
/// the next command and the next editor session start warm too.
fn load_index(root: &Path, config: &Config) -> Index {
    let cache = config
        .index
        .cache
        .then_some(config.index.cache_dir.as_path());
    let mut index = cache
        .and_then(|directory| frappe_index::cache::load(root, directory))
        .unwrap_or_else(|| Index::build(root));
    if !index.occurrences().scanned {
        index.scan_occurrences();
        if let Some(directory) = cache {
            // Costs a slower start next time and nothing else.
            let _ = frappe_index::cache::save(&index, root, directory);
        }
    }
    index
}

fn check(paths: &[String], json: bool) -> Result<ExitCode, String> {
    let current = std::env::current_dir().map_err(|error| error.to_string())?;
    let targets: Vec<PathBuf> = if paths.is_empty() {
        vec![current.clone()]
    } else {
        paths.iter().map(|path| current.join(path)).collect()
    };
    if let Some(missing) = targets.iter().find(|target| !target.exists()) {
        return Err(format!("{} does not exist", missing.display()));
    }

    let bench = Bench::open(&targets[0])?;
    let files: Vec<PathBuf> = targets
        .iter()
        .flat_map(|target| checkable(target))
        .collect();
    let findings = findings(&bench, &files);

    if json {
        let report: Vec<Value> = findings
            .iter()
            .map(|(path, diagnostic)| finding_json(&shown(path, &current), diagnostic))
            .collect();
        println!("{}", json!({"checked": files.len(), "findings": report}));
    } else {
        for (path, diagnostic) in &findings {
            println!("{}", finding_line(&shown(path, &current), diagnostic));
        }
        eprintln!("{}", summary(findings.len(), files.len()));
    }
    Ok(if findings.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(FOUND)
    })
}

/// The files under `target` a check covers. A file named outright is checked
/// whatever it is; a directory contributes what the server would open.
fn checkable(target: &Path) -> Vec<PathBuf> {
    if !target.is_dir() {
        return vec![target.to_path_buf()];
    }
    let mut files: Vec<PathBuf> = frappe_index::walk::Walk::new(target)
        .filter(|path| {
            path.file_name().is_some_and(|name| name == "patches.txt")
                || path.extension().is_some_and(|extension| {
                    ["py", "js", "json", "html"]
                        .iter()
                        .any(|known| extension == *known)
                })
        })
        .collect();
    files.sort();
    files
}

/// Every diagnostic in `files`, in path then position order. Parsing is the
/// cost, and files are independent, so they are split across threads.
fn findings(bench: &Bench, files: &[PathBuf]) -> Vec<(PathBuf, Diagnostic)> {
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let chunk = files.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let workers: Vec<_> = files
            .chunks(chunk)
            .map(|part| scope.spawn(move || diagnose(bench, part)))
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap_or_default())
            .collect()
    })
}

fn diagnose(bench: &Bench, files: &[PathBuf]) -> Vec<(PathBuf, Diagnostic)> {
    let mut found = Vec::new();
    for path in files {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let diagnostics = crate::diagnostics::for_buffer(
            &bench.index,
            &bench.config,
            path,
            &source,
            PositionEncoding::Utf8,
        );
        found.extend(
            diagnostics
                .into_iter()
                .map(|diagnostic| (path.clone(), diagnostic)),
        );
    }
    found
}

/// `path:line:column: severity: message [code]`, 1-based, the column in bytes.
fn finding_line(path: &str, diagnostic: &Diagnostic) -> String {
    let start = diagnostic.range.start;
    format!(
        "{path}:{}:{}: {}: {} [{}]",
        start.line + 1,
        start.character + 1,
        severity_name(diagnostic.severity),
        diagnostic.message,
        code_of(diagnostic),
    )
}

fn finding_json(path: &str, diagnostic: &Diagnostic) -> Value {
    let range = diagnostic.range;
    json!({
        "path": path,
        "line": range.start.line + 1,
        "column": range.start.character + 1,
        "endLine": range.end.line + 1,
        "endColumn": range.end.character + 1,
        "severity": severity_name(diagnostic.severity),
        "code": code_of(diagnostic),
        "message": diagnostic.message,
    })
}

fn summary(findings: usize, files: usize) -> String {
    let plural = |count: usize, word: &str| {
        if count == 1 {
            format!("{count} {word}")
        } else {
            format!("{count} {word}s")
        }
    };
    format!(
        "{} in {} checked",
        plural(findings, "finding"),
        plural(files, "file")
    )
}

fn code_of(diagnostic: &Diagnostic) -> String {
    match &diagnostic.code {
        Some(NumberOrString::String(code)) => code.clone(),
        Some(NumberOrString::Number(code)) => code.to_string(),
        None => String::new(),
    }
}

fn severity_name(severity: Option<DiagnosticSeverity>) -> &'static str {
    match severity {
        Some(DiagnosticSeverity::ERROR) => "error",
        Some(DiagnosticSeverity::INFORMATION) => "info",
        Some(DiagnosticSeverity::HINT) => "hint",
        _ => "warning",
    }
}

fn describe(doctype: &str, json: bool) -> Result<ExitCode, String> {
    let current = std::env::current_dir().map_err(|error| error.to_string())?;
    let bench = Bench::open(&current)?;
    let described = crate::agent::describe(&bench.index, doctype);
    if json {
        println!("{described}");
    } else if described["found"] == true {
        print!("{}", schema_text(&described, &current));
    } else {
        eprintln!("{}", not_found(doctype, &described["similar"]));
    }
    Ok(if described["found"] == true {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(FOUND)
    })
}

/// A header, then one field per line: the shape a model reads fastest.
fn schema_text(described: &Value, current: &Path) -> String {
    let flag = |key: &str| described[key] == true;
    let kinds: Vec<&str> = [
        ("isChild", "child table"),
        ("isSingle", "single"),
        ("isSubmittable", "submittable"),
        ("isVirtual", "virtual"),
        ("isTree", "tree"),
    ]
    .into_iter()
    .filter(|(key, _)| flag(key))
    .map(|(_, name)| name)
    .collect();

    let mut text = format!(
        "{} — {} · {}{}\n",
        described["name"].as_str().unwrap_or_default(),
        described["app"].as_str().unwrap_or("site"),
        described["module"].as_str().unwrap_or_default(),
        if kinds.is_empty() {
            String::new()
        } else {
            format!(" · {}", kinds.join(", "))
        },
    );
    if let Some(path) = described["path"].as_str() {
        text.push_str(&format!("schema: {}\n", shown(Path::new(path), current)));
    }
    if let Some(path) = described["controller"]["path"].as_str() {
        text.push_str(&format!(
            "controller: {}\n",
            shown(Path::new(path), current)
        ));
    }
    if let Some(title) = described["titleField"].as_str() {
        text.push_str(&format!("title field: {title}\n"));
    }
    for field in described["fields"].as_array().into_iter().flatten() {
        text.push_str(&field_text(field));
    }
    text
}

fn field_text(field: &Value) -> String {
    let mut line = format!(
        "  {}  {}",
        field["fieldname"].as_str().unwrap_or_default(),
        field["fieldtype"].as_str().unwrap_or_default()
    );
    if let Some(options) = field["options"]
        .as_str()
        .filter(|options| !options.is_empty())
    {
        line.push_str(&format!(" → {}", options.replace('\n', " | ")));
    }
    for (key, note) in [
        ("reqd", "required"),
        ("read_only", "read only"),
        ("hidden", "hidden"),
    ] {
        if field[key] == true {
            line.push_str(&format!(" ({note})"));
        }
    }
    if field["layer"] != "schema" {
        line.push_str(&format!(
            " [{}]",
            field["layer"].as_str().unwrap_or_default()
        ));
    }
    line.push('\n');
    line
}

fn references(doctype: &str, fieldname: Option<&str>, json: bool) -> Result<ExitCode, String> {
    let current = std::env::current_dir().map_err(|error| error.to_string())?;
    let bench = Bench::open(&current)?;
    if let Some(problem) = unknown_name(&bench.index, doctype, fieldname) {
        eprintln!("{problem}");
        return Ok(ExitCode::from(FOUND));
    }

    let found = crate::agent::references(&bench.index, doctype, fieldname);
    if json {
        println!("{found}");
        return Ok(ExitCode::SUCCESS);
    }
    let sites = found["sites"].as_array().cloned().unwrap_or_default();
    for site in &sites {
        println!(
            "{}:{}: {}",
            shown(
                Path::new(site["path"].as_str().unwrap_or_default()),
                &current
            ),
            site["line"],
            site["text"].as_str().unwrap_or_default()
        );
    }
    let total = found["total"].as_u64().unwrap_or_default();
    if usize::try_from(total).is_ok_and(|total| total > sites.len()) {
        eprintln!("{total} references; the first {} shown", sites.len());
    } else {
        eprintln!("{total} references");
    }
    eprintln!("{}", crate::agent::REFERENCES_COVER);
    Ok(ExitCode::SUCCESS)
}

/// A name the index does not know, said as such: an empty list would read as
/// "nothing uses this".
fn unknown_name(index: &Index, doctype: &str, fieldname: Option<&str>) -> Option<String> {
    if index.doctype(doctype).is_none() {
        let similar = json!(index.case_insensitive_matches(doctype));
        return Some(not_found(doctype, &similar));
    }
    let fieldname = fieldname?;
    (!index.has_field(doctype, fieldname))
        .then(|| format!("`{doctype}` has no field `{fieldname}`"))
}

fn not_found(doctype: &str, similar: &Value) -> String {
    let similar: Vec<&str> = similar
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if similar.is_empty() {
        format!("no DocType named `{doctype}`")
    } else {
        format!(
            "no DocType named `{doctype}`; did you mean {}?",
            similar.join(", ")
        )
    }
}

/// A path as the caller would type it: relative below the current directory.
fn shown(path: &Path, current: &Path) -> String {
    path.strip_prefix(current)
        .unwrap_or(path)
        .display()
        .to_string()
}
