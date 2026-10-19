//! `cargo xtask smoke` — [32](../../docs/specs/32-testing.md).
//!
//! Frappe's own code is the best available corpus of valid Frappe code, so any
//! diagnostic it produces is by default a false positive. This job is what
//! turns [23](../../docs/specs/23-diagnostics.md)'s precision rule into
//! something enforced rather than measured by hand once and then trusted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use frappe_index::Index;
use frappe_lsp::config::Config;
use frappe_lsp::diagnostics;
use serde::Deserialize;

/// A finding the corpus is known to produce, and why it is allowed.
#[derive(Debug, Deserialize)]
struct Allowed {
    /// Path relative to the bench root.
    file: String,
    /// The code without the `frappe-lsp/` prefix.
    code: String,
    /// The diagnostic message, verbatim. Matching on it rather than on the
    /// code keeps a second finding in an already-listed file from slipping
    /// through: `hooks.py` alone holds thirteen allowed `unknown-hook` lines,
    /// and a fourteenth is news.
    message: String,
    /// How many times it fires in that file. An eleventh occurrence of a
    /// message allowed ten times is news.
    #[serde(default = "one")]
    count: usize,
    /// Why this one is right about code that is wrong, or unknowable.
    #[allow(dead_code)]
    reason: String,
}

#[derive(Debug, Default, Deserialize)]
struct Allowlist {
    #[serde(default, rename = "allow")]
    entries: Vec<Allowed>,
}

fn one() -> usize {
    1
}

impl Allowlist {
    /// How many times this exact finding is allowed in this exact file.
    fn budget(&self, finding: &Finding) -> usize {
        self.entries
            .iter()
            .filter(|entry| {
                entry.file == finding.0 && entry.code == finding.1 && entry.message == finding.2
            })
            .map(|entry| entry.count)
            .sum()
    }
}

/// File, code, message — the identity of a finding for allowlist purposes.
type Finding = (String, String, String);

/// Run the corpus check. Returns the number of findings that were not allowed.
pub fn run(
    bench: &Path,
    allowlist: &Path,
    write: bool,
    all: bool,
    absence: bool,
) -> std::io::Result<usize> {
    let started = Instant::now();
    let index = Index::build(bench);
    let status = index.status();
    println!(
        "indexed {} apps, {} DocTypes, {} fields in {:?}",
        status.apps,
        status.doctypes,
        status.fields,
        started.elapsed()
    );
    if status.doctypes == 0 {
        eprintln!("no DocTypes indexed — is {} a bench?", bench.display());
        return Ok(1);
    }

    let allowed: Allowlist = std::fs::read_to_string(allowlist)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default();

    // The absence checks ship off, so measuring them means asking for them:
    // `--absence` is the loop that decides whether they can ever ship on
    // ([23](../../docs/specs/23-diagnostics.md)).
    // `unknown-attribute` ships off: a Document is an open Python object, and
    // deciding whether `doc.x` is a typo needs type information this server
    // does not have. `--absence` turns it on to keep the number measured
    // rather than assumed ([23](../../docs/specs/23-diagnostics.md)).
    // An app outside the canonical set changes what the corpus reports: it can
    // define a DocType erpnext links to, which silences a finding, and it can
    // redefine one, which invents a conflict on erpnext's own file. The first
    // shows up as a stale allowlist entry and is harmless; the second would
    // write one developer's bench into a committed list, so it is dropped.
    let foreign: Vec<&str> = index
        .apps()
        .iter()
        .map(|app| app.name.as_str())
        .filter(|name| !CANONICAL.contains(name))
        .collect();
    let ignore_conflicts = !foreign.is_empty() && !all;
    if ignore_conflicts {
        println!(
            "note: {} also installed; conflict findings on canonical files are ignored, and \
             the committed allowlist is written against a bench holding {} alone",
            foreign.join(", "),
            CANONICAL.join(" + "),
        );
    }

    let mut config = Config::default();
    if absence {
        config.diagnostics.unknown_attribute = frappe_lsp::config::Level::Warning;
    }
    let mut observed: BTreeMap<Finding, usize> = BTreeMap::new();
    // The first line each finding lands on, so triage starts at the source
    // rather than at a grep.
    let mut lines: BTreeMap<Finding, u32> = BTreeMap::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut files = 0;

    for path in corpus(&index, all) {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        files += 1;
        let relative = path
            .strip_prefix(bench)
            .unwrap_or(&path)
            .display()
            .to_string();

        for diagnostic in diagnostics::for_buffer(&index, &config, &path, &source) {
            let Some(code) = code_of(&diagnostic) else {
                continue;
            };
            if ignore_conflicts
                && matches!(
                    code.as_str(),
                    "duplicate-doctype" | "custom-field-collision"
                )
            {
                continue;
            }
            *counts.entry(code.clone()).or_default() += 1;
            lines
                .entry((relative.clone(), code.clone(), diagnostic.message.clone()))
                .or_insert(diagnostic.range.start.line + 1);
            *observed
                .entry((relative.clone(), code, diagnostic.message.clone()))
                .or_default() += 1;
        }
    }

    println!("{files} files checked in {:?}", started.elapsed());
    for (code, count) in &counts {
        println!("  {count:5}  {code}");
    }

    if write {
        let mut out = String::from(
            "# Generated by `cargo xtask smoke --write`, then reviewed by hand.\n\
             #\n\
             # Every entry is a diagnostic that is right about code that is wrong, or\n\
             # right about something no static index can see. An entry without a reason\n\
             # is a false positive wearing a disguise, and the list must never grow to\n\
             # accommodate a noisy check — see docs/specs/32-testing.md.\n\n",
        );
        for ((file, code, message), count) in &observed {
            out.push_str(&format!(
                "[[allow]]\nfile = {}\ncode = {}\nmessage = {}\ncount = {count}\nreason = {}\n\n",
                quote(file),
                quote(code),
                quote(message),
                quote(reason(code)),
            ));
        }
        std::fs::write(allowlist, out)?;
        println!(
            "wrote {} entries to {}",
            observed.len(),
            allowlist.display()
        );
        return Ok(0);
    }

    let mut unexpected: Vec<(Finding, usize)> = Vec::new();
    for (finding, count) in &observed {
        let budget = allowed.budget(finding);
        if *count > budget {
            unexpected.push((finding.clone(), count - budget));
        }
    }

    // An entry that no longer fires is not a failure — the corpus moves — but
    // it is dead weight, and a list nobody prunes stops being read.
    let stale: Vec<&Allowed> = allowed
        .entries
        .iter()
        .filter(|entry| {
            !observed.contains_key(&(
                entry.file.clone(),
                entry.code.clone(),
                entry.message.clone(),
            ))
        })
        .collect();
    if !stale.is_empty() {
        println!("\n{} allowlist entries no longer fire:", stale.len());
        for entry in stale {
            println!("  {} — {}", entry.file, entry.code);
        }
    }

    if unexpected.is_empty() {
        println!("clean: every finding is on the allowlist");
        return Ok(0);
    }

    let total: usize = unexpected.iter().map(|(_, over)| over).sum();
    eprintln!("\n{total} findings not on the allowlist:");
    for (finding, over) in &unexpected {
        let (file, code, message) = finding;
        let line = lines.get(finding).copied().unwrap_or(0);
        eprintln!("  {file}:{line}\n    {code} (x{over}): {message}");
    }
    Ok(total)
}

/// The apps the allowlist is written against. A developer's bench also holds
/// their own apps, and findings there are theirs, not this repository's.
const CANONICAL: [&str; 2] = ["frappe", "erpnext"];

/// Every file the checks read, across the canonical apps.
fn corpus(index: &Index, all: bool) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for app in index.apps() {
        if !all && !CANONICAL.contains(&app.name.as_str()) {
            continue;
        }
        for path in frappe_index::walk::Walk::new(&app.package) {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let wanted = matches!(extension, "py" | "js")
                || name == "patches.txt"
                || (extension == "json"
                    && path.file_stem() == path.parent().and_then(Path::file_name));
            if wanted {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// The diagnostic code, without the `frappe-lsp/` prefix.
fn code_of(diagnostic: &tower_lsp_server::ls_types::Diagnostic) -> Option<String> {
    match diagnostic.code.as_ref()? {
        tower_lsp_server::ls_types::NumberOrString::String(code) => {
            Some(code.trim_start_matches("frappe-lsp/").to_string())
        }
        tower_lsp_server::ls_types::NumberOrString::Number(_) => None,
    }
}

/// A TOML basic string.
fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Why a class of finding is allowed. Written per code because that is the
/// level the judgement is actually made at; the message names which one.
fn reason(code: &str) -> &'static str {
    match code {
        "translation-literal" => {
            "an f-string, a concatenation or .format() inside _(): real, and the \
             string genuinely never reaches the translation catalogue"
        }
        "unknown-hook" => {
            "no reader anywhere in the bench — dead config kept for a frappe \
             version that no longer looks for it"
        }
        "dangling-depends-on" => {
            "names a field no schema declares: either a leftover, or a Customize \
             Form field only the live site can see, which is what Static \
             confidence means"
        }
        "unresolvable-fetch-from" => {
            "the source field is absent from the linked DocType, so the fetch \
             silently stays empty"
        }
        "unresolvable-patch" | "duplicate-patch" => {
            "patches.txt names a module that is not in the checkout, or names one \
             twice"
        }
        "unresolvable-path" => {
            "the dotted path defines no such attribute; frappe's own \
             override_whitelisted_methods carries two of these"
        }
        "missing-asset" => {
            "the asset is not a file in the app: dead config, or served from a \
             DocType at runtime"
        }
        "unknown-link-target" => {
            "the target DocType belongs to an app this bench does not have \
             installed"
        }
        "table-not-child" => "the target DocType is not marked istable",
        "unknown-doctype" => {
            "no DocType of that name in this bench: erpnext links into apps \
             this checkout does not install, and a name that resolves nowhere \
             is what Static confidence can see"
        }
        "unknown-field" => {
            "a fieldname written as a string, in a position frappe queries, \
             that no schema declares: either a dead reference that silently \
             returns nothing, or a Custom Field or runtime attribute that code \
             creates and only a live site can show, which is what Static \
             confidence means"
        }
        "parent-field-on-child" => {
            "a row given a field only its parent declares: frappe drops it from \
             the row, so the value never lands"
        }
        "unknown-form-event" => {
            "a handler that is not a lifecycle event, not a fieldname, not a \
             grid event, and that nothing triggers: it never fires"
        }
        _ => "REVIEW: no class reason recorded — decide before committing",
    }
}
