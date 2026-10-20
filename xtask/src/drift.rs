//! `cargo xtask codegen --check` — the vendored tables against a `frappe`
//! checkout.
//!
//! The tables in `frappe-syntax/src/generated.rs` are written by hand: the
//! hook names can be scraped, but what each hook's value names is a judgement.
//! What can be automated is noticing that frappe reads a name the table lacks.
//! Every such gap has surfaced as a false positive on correct code or a path
//! that navigates nowhere, so a missing name fails the check.

use std::collections::BTreeSet;
use std::path::Path;

use frappe_syntax::generated;
use regex::Regex;

/// Compare every table with what `frappe` reads. Returns how many names the
/// tables lack.
pub fn run(frappe: &Path) -> std::io::Result<usize> {
    let package = frappe.join("frappe");
    if !package.join("hooks.py").is_file() {
        return Err(std::io::Error::other(format!(
            "{} is not a frappe checkout",
            frappe.display()
        )));
    }
    let python = files(&package, "py");

    let hooks = hook_names(&package, &python);
    let known: BTreeSet<&str> = generated::HOOKS.iter().map(|(name, _)| *name).collect();
    let mut missing = report("hook", &hooks, &known);

    let events = scrape(
        &[package.join("model/document.py")],
        &[r#"run_method\(\s*["']([a-z_]+)["']"#],
    );
    let known: BTreeSet<&str> = generated::DOC_EVENTS.iter().copied().collect();
    missing += report("doc event", &events, &known);

    let form = files(&package.join("public/js/frappe/form"), "js");
    let triggered = scrape(
        &form,
        &[r#"(?:script_manager|frm|this)\.trigger\(\s*["']([a-z_]+)["']\s*\)"#],
    );
    let known: BTreeSet<&str> = generated::FORM_LIFECYCLE_EVENTS.iter().copied().collect();
    missing += report("form event", &triggered, &known);

    let members = document_members(&[
        (package.join("model/base_document.py"), "BaseDocument"),
        (package.join("model/document.py"), "Document"),
    ]);
    let known: BTreeSet<&str> = frappe_index::fieldtypes::DOCUMENT_MEMBERS
        .iter()
        .copied()
        .collect();
    missing += report("Document member", &members, &known);

    Ok(missing)
}

/// What `doc.<name>` reaches on every document: the methods, properties and
/// class attributes of `Document` and `BaseDocument`, and what their methods
/// assign to `self`. A name that is also an implicit field is a field first.
fn document_members(classes: &[(std::path::PathBuf, &str)]) -> BTreeSet<String> {
    let mut parser = frappe_syntax::Language::Python
        .parser()
        .expect("the Python grammar");
    let mut found = BTreeSet::new();
    for (path, class) in classes {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };
        let text = |node: tree_sitter::Node<'_>| source[node.byte_range()].to_string();
        let root = tree.root_node();
        let mut cursor = root.walk();
        let Some(body) = root
            .named_children(&mut cursor)
            .find(|node| {
                node.kind() == "class_definition"
                    && node
                        .child_by_field_name("name")
                        .is_some_and(|name| text(name) == *class)
            })
            .and_then(|node| node.child_by_field_name("body"))
        else {
            continue;
        };

        let mut stack = vec![body];
        while let Some(node) = stack.pop() {
            let mut inner = node.walk();
            stack.extend(node.named_children(&mut inner));
            let name = match node.kind() {
                // `def name` directly in the class, decorated or not.
                "function_definition"
                    if node.parent().is_some_and(|parent| {
                        parent.id() == body.id()
                            || parent.kind() == "decorated_definition"
                                && parent.parent().is_some_and(|outer| outer.id() == body.id())
                    }) =>
                {
                    node.child_by_field_name("name").map(text)
                }
                // `name = …` in the class body, and `self.name = …` anywhere in it.
                "assignment" => {
                    node.child_by_field_name("left")
                        .and_then(|left| match left.kind() {
                            "identifier"
                                if node
                                    .parent()
                                    .and_then(|parent| parent.parent())
                                    .is_some_and(|outer| outer.id() == body.id()) =>
                            {
                                Some(text(left))
                            }
                            "attribute"
                                if left
                                    .child_by_field_name("object")
                                    .is_some_and(|object| text(object) == "self") =>
                            {
                                left.child_by_field_name("attribute").map(text)
                            }
                            _ => None,
                        })
                }
                _ => None,
            };
            if let Some(name) = name.filter(|name| {
                // Every `__` name is a member to the index already: dunders,
                // and `self.__x`, which Python mangles out of reach.
                !name.starts_with("__")
                    && !frappe_index::fieldtypes::is_implicit(name, false)
                    && !frappe_index::fieldtypes::is_implicit(name, true)
            }) {
                found.insert(name);
            }
        }
    }
    found
}

/// Every name frappe reads as a hook. No single source is the list: the
/// recipe beside `HOOKS` names four, and `boot_session` showed a fifth.
fn hook_names(package: &Path, python: &[std::path::PathBuf]) -> BTreeSet<String> {
    let mut names = scrape(
        python,
        &[
            // `get_hooks("name")`
            r#"get_hooks\(\s*["']([a-z_][a-z0-9_]*)["']"#,
            // `get_hooks(app_name=…).get("name")`
            r#"get_hooks\([^)"']*\)\s*\.get\(\s*["']([a-z_][a-z0-9_]*)["']"#,
            // `get_code_files_via_hooks("doctype_js", …)` and its siblings
            r#"via_hooks?\(\s*["']([a-z_][a-z0-9_]*)["']"#,
        ],
    );
    names.extend(scrape(
        &[package.join("hooks.py")],
        &[r"(?m)^([a-z_][a-z0-9_]*)\s*="],
    ));

    // `hooks = frappe.get_hooks()`, then `hooks.boot_session`,
    // `hooks.get("name")` or `hooks["name"]` further down the same file.
    let bound = Regex::new(r"(?m)^\s*(\w+)\s*=\s*frappe\.get_hooks\(\s*(?:app_name\s*=[^)]*)?\)")
        .expect("pattern");
    const DICT: [&str; 8] = [
        "get",
        "items",
        "keys",
        "values",
        "setdefault",
        "copy",
        "update",
        "pop",
    ];
    let mut parser = frappe_syntax::Language::Python
        .parser()
        .expect("the Python grammar");
    for path in python {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let variables: BTreeSet<String> = bound
            .captures_iter(&source)
            .map(|found| found[1].to_string())
            .collect();
        if variables.is_empty() {
            continue;
        }
        // Parsed rather than matched: a docstring saying "in hooks.py" is not
        // a read of a hook called `py`.
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            let mut cursor = node.walk();
            stack.extend(node.named_children(&mut cursor));
            let read = match node.kind() {
                // `hooks.boot_session`
                "attribute" => node
                    .child_by_field_name("object")
                    .zip(node.child_by_field_name("attribute")),
                // `hooks["name"]`, and not `hooks[0]` or `hooks[doctype]`
                "subscript" => node.child_by_field_name("value").zip(
                    node.child_by_field_name("subscript")
                        .filter(|key| key.kind() == "string"),
                ),
                _ => None,
            };
            let Some((object, name)) = read else {
                continue;
            };
            if !variables.contains(&source[object.byte_range()]) {
                continue;
            }
            let name = source[name.byte_range()].trim_matches(['"', '\'']);
            let plain = name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if plain && !DICT.contains(&name) {
                names.insert(name.to_string());
            }
        }
        // `hooks.get("name")`
        for variable in &variables {
            let gets = Regex::new(&format!(
                r#"\b{variable}\.get\(\s*["']([a-z_][a-z0-9_]*)["']"#
            ))
            .expect("pattern");
            names.extend(
                gets.captures_iter(&source)
                    .map(|found| found[1].to_string()),
            );
        }
    }
    names
}

fn report(kind: &str, found: &BTreeSet<String>, known: &BTreeSet<&str>) -> usize {
    let missing: Vec<&String> = found
        .iter()
        .filter(|name| !known.contains(name.as_str()))
        .collect();
    let stale: Vec<&&str> = known
        .iter()
        .filter(|name| !found.contains(**name))
        .collect();
    println!(
        "{kind}s: {} read by frappe, {} in the table, {} missing",
        found.len(),
        known.len(),
        missing.len()
    );
    for name in &missing {
        println!("  missing {kind}: {name}");
    }
    // Informational: an app may define what frappe no longer reads, and the
    // table keeps it so their `hooks.py` is not accused.
    if !stale.is_empty() {
        println!("  {} in the table that frappe no longer reads", stale.len());
    }
    missing.len()
}

fn scrape(paths: &[std::path::PathBuf], patterns: &[&str]) -> BTreeSet<String> {
    let patterns: Vec<Regex> = patterns
        .iter()
        .map(|pattern| Regex::new(pattern).expect("pattern"))
        .collect();
    let mut found = BTreeSet::new();
    for path in paths {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        for pattern in &patterns {
            for capture in pattern.captures_iter(&source) {
                if let Some(name) = capture.get(1) {
                    found.insert(name.as_str().to_string());
                }
            }
        }
    }
    found
}

/// Source files under `root`, tests excluded: a test patches hooks it invents.
fn files(root: &Path, extension: &str) -> Vec<std::path::PathBuf> {
    frappe_index::walk::Walk::new(root)
        .filter(|path| path.extension().is_some_and(|found| found == extension))
        .filter(|path| {
            !path.components().any(|part| part.as_os_str() == "tests")
                && !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("test_"))
        })
        .collect()
}
