//! Hooks resolved against the index — [12](../../../docs/specs/12-surface-hooks.md).
//!
//! Extraction reads the literals; this says what they name. Two jobs, and the
//! second is the one the rest of the index waits on: `override_doctype_class`
//! decides which class a DocType's controller is, `override_whitelisted_methods`
//! decides where a `frappe.call` lands, and `doctype_js` binds a form script to
//! a DocType it does not sit beside. Spec 12 is explicit that `hooks.py` is
//! therefore indexed before any JS or template in the same app.

use std::path::{Path, PathBuf};

use frappe_syntax::generated::{self, Schema};
use frappe_syntax::hooks::{Assignment, Value};
use frappe_syntax::matches::Span;

/// One hook value, and what it turned out to name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HookAssignment {
    /// The hook name.
    pub key: String,
    /// Where the name is written.
    pub key_span: Span,
    /// Its schema, or `None` for a name `frappe` never reads.
    pub schema: Option<Schema>,
    /// The app this `hooks.py` belongs to.
    pub app: String,
    /// The file.
    pub path: PathBuf,
    /// Whether the value was a literal at all. A computed value is present and
    /// unvalidated — spec 12.
    pub opaque: bool,
}

/// What one app's `hooks.py` contributes to the index.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Hooks {
    /// Every module-level assignment, in file order.
    pub assignments: Vec<HookAssignment>,
    /// `override_doctype_class`: DocType name → class path.
    pub overrides: Vec<(String, String)>,
    /// `extend_doctype_class`: DocType name → mixin class paths.
    pub mixins: Vec<(String, Vec<String>)>,
    /// `override_whitelisted_methods`: original path → effective path.
    pub remapped: Vec<(String, String)>,
    /// `doctype_js` and friends: DocType name → asset path as written.
    pub scripts: Vec<(String, String)>,
    /// `jinja.methods` and `jinja.filters`.
    pub jinja: Vec<String>,
}

/// Read one app's `hooks.py`.
///
/// A file that cannot be read or parsed yields nothing; an app with no usable
/// hooks still indexes ([02](../../../docs/specs/02-architecture.md)).
#[must_use]
pub fn read(path: &Path, app: &str) -> Hooks {
    let Ok(source) = std::fs::read_to_string(path) else {
        return Hooks::default();
    };
    let Ok(assignments) = frappe_syntax::hooks::read(&source) else {
        return Hooks::default();
    };
    from_assignments(&assignments, path, app)
}

/// The cross-hook effects, without touching the filesystem — the unit-testable
/// half.
#[must_use]
pub fn from_assignments(assignments: &[Assignment], path: &Path, app: &str) -> Hooks {
    let mut hooks = Hooks::default();

    for assignment in assignments {
        let schema = generated::hook_schema(&assignment.key);
        hooks.assignments.push(HookAssignment {
            key: assignment.key.clone(),
            key_span: assignment.key_span,
            schema,
            app: app.to_string(),
            path: path.to_path_buf(),
            opaque: matches!(assignment.value, Value::Opaque(_)),
        });

        match assignment.key.as_str() {
            "override_doctype_class" => {
                for (doctype, value) in assignment.value.entries() {
                    if let Some((class, _)) = value.texts().first() {
                        hooks
                            .overrides
                            .push((doctype.text.clone(), (*class).to_string()));
                    }
                }
            }
            "extend_doctype_class" => {
                for (doctype, value) in assignment.value.entries() {
                    let classes: Vec<String> = value
                        .texts()
                        .into_iter()
                        .map(|(text, _)| text.to_string())
                        .collect();
                    if !classes.is_empty() {
                        hooks.mixins.push((doctype.text.clone(), classes));
                    }
                }
            }
            "override_whitelisted_methods" => {
                for (original, value) in assignment.value.entries() {
                    if let Some((effective, _)) = value.texts().first() {
                        hooks
                            .remapped
                            .push((original.text.clone(), (*effective).to_string()));
                    }
                }
            }
            // The four script hooks bind a file to a DocType that it need not
            // sit beside, which is the only way to find it.
            "doctype_js" | "doctype_list_js" | "doctype_tree_js" | "doctype_calendar_js" => {
                for (doctype, value) in assignment.value.entries() {
                    for (asset, _) in value.texts() {
                        hooks
                            .scripts
                            .push((doctype.text.clone(), asset.to_string()));
                    }
                }
            }
            "jinja" => {
                for (_, value) in assignment.value.entries() {
                    hooks
                        .jinja
                        .extend(value.texts().into_iter().map(|(text, _)| text.to_string()));
                }
            }
            _ => {}
        }
    }

    hooks
}

impl Hooks {
    /// Assignments to the same key. Spec 12: the second silently wins, so the
    /// duplicate is only visible if both were kept.
    #[must_use]
    pub fn duplicates(&self) -> Vec<&str> {
        let mut seen: Vec<&str> = Vec::new();
        let mut repeated: Vec<&str> = Vec::new();
        for assignment in &self.assignments {
            let key = assignment.key.as_str();
            if seen.contains(&key) {
                if !repeated.contains(&key) {
                    repeated.push(key);
                }
            } else {
                seen.push(key);
            }
        }
        repeated
    }

    /// Names assigned that `frappe` never reads.
    #[must_use]
    pub fn unknown_keys(&self) -> Vec<&str> {
        self.assignments
            .iter()
            .filter(|assignment| assignment.schema.is_none())
            .map(|assignment| assignment.key.as_str())
            .collect()
    }

    /// The effective target of a whitelisted path, following one remap.
    #[must_use]
    pub fn effective_method<'a>(&'a self, dotted: &'a str) -> &'a str {
        self.remapped
            .iter()
            .find(|(original, _)| original == dotted)
            .map_or(dotted, |(_, effective)| effective.as_str())
    }

    /// Form scripts bound to a DocType from elsewhere in the app.
    #[must_use]
    pub fn scripts_for(&self, doctype: &str) -> Vec<&str> {
        self.scripts
            .iter()
            .filter(|(name, _)| name == doctype)
            .map(|(_, asset)| asset.as_str())
            .collect()
    }
}

/// Resolve an asset path as written in a hook, against the app package.
///
/// Spec 07's three forms. A generated bundle under `sites/assets/` is never the
/// answer: navigation targets the app source it was built from.
#[must_use]
pub fn asset_path(package: &Path, app: &str, written: &str) -> Option<PathBuf> {
    let relative = if let Some(rest) = written.strip_prefix("/assets/") {
        // `/assets/<app>/js/x.js` reaches the package through the symlink.
        let rest = rest.strip_prefix(app)?.trim_start_matches('/');
        format!("public/{rest}")
    } else if let Some(rest) = written.strip_prefix(&format!("{app}/")) {
        rest.to_string()
    } else {
        written.to_string()
    };

    let candidate = package.join(&relative);
    if candidate.is_file() {
        return Some(candidate);
    }

    // A bare `desk.bundle.js` is spec 07's third form: the build resolves it
    // through the assets manifest, and the source it was built from lives in
    // the app. Navigation targets that source, never the bundle.
    //
    // A `.css` bundle is built from `.scss`, so the name in the hook exists
    // nowhere on disk — only the stylesheet it is compiled from does.
    if !relative.contains('/') {
        let candidates = [
            package.join("public/js").join(&relative),
            package.join("public/css").join(&relative),
            package
                .join("public/scss")
                .join(relative.replace(".css", ".scss")),
        ];
        return candidates.into_iter().find(|path| path.is_file());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hooks(source: &str) -> Hooks {
        let assignments = frappe_syntax::hooks::read(source).expect("the grammar loads");
        from_assignments(&assignments, Path::new("/bench/apps/x/x/hooks.py"), "x")
    }

    #[test]
    fn override_doctype_class_reaches_the_index() {
        let hooks = hooks("override_doctype_class = {\"Task\": \"app.overrides.CustomTask\"}\n");
        assert_eq!(
            hooks.overrides,
            [("Task".to_string(), "app.overrides.CustomTask".to_string())]
        );
    }

    #[test]
    fn extend_doctype_class_collects_every_mixin() {
        let hooks = hooks("extend_doctype_class = {\"Task\": [\"app.a.One\", \"app.b.Two\"]}\n");
        assert_eq!(hooks.mixins[0].1, ["app.a.One", "app.b.Two"]);
    }

    #[test]
    fn a_remapped_method_reports_its_effective_target() {
        let hooks =
            hooks("override_whitelisted_methods = {\"frappe.client.get\": \"app.api.get\"}\n");
        assert_eq!(hooks.effective_method("frappe.client.get"), "app.api.get");
        // Anything unremapped is its own target.
        assert_eq!(hooks.effective_method("app.api.other"), "app.api.other");
    }

    #[test]
    fn script_hooks_bind_files_to_doctypes() {
        let hooks = hooks(
            "doctype_js = {\"Task\": \"public/js/task.js\"}\n\
             doctype_list_js = {\"Task\": [\"public/js/task_list.js\"]}\n",
        );
        assert_eq!(
            hooks.scripts_for("Task"),
            ["public/js/task.js", "public/js/task_list.js"]
        );
    }

    #[test]
    fn an_unknown_key_is_named_and_a_known_one_is_not() {
        let hooks = hooks("doctype_javascript = {}\ndoctype_js = {}\n");
        assert_eq!(hooks.unknown_keys(), ["doctype_javascript"]);
    }

    #[test]
    fn a_duplicate_assignment_is_reported_once() {
        let hooks = hooks("after_install = \"a.b\"\nafter_install = \"a.c\"\n");
        assert_eq!(hooks.duplicates(), ["after_install"]);
    }

    #[test]
    fn a_computed_value_is_recorded_as_opaque() {
        let hooks = hooks("standard_queries = {dt: h for dt in x}\n");
        assert!(hooks.assignments[0].opaque);
        // Present, and nothing resolves against it.
        assert!(hooks.overrides.is_empty());
    }

    #[test]
    fn asset_paths_resolve_in_both_documented_forms() {
        let package = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repo root")
            .join("tests/fixtures/bench/apps/library/library");

        assert!(asset_path(&package, "library", "public/js/library.js").is_some());
        assert!(asset_path(&package, "library", "/assets/library/js/library.js").is_some());
        // A file that is not there resolves to nothing rather than to a guess.
        assert!(asset_path(&package, "library", "/assets/library/js/missing.js").is_none());
    }
}

/// One line of a `patches.txt` — [07](../../../docs/specs/07-dotted-paths.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    /// The module path as written.
    pub dotted: String,
    /// Byte range of the path within the file.
    pub span: (usize, usize),
    /// Whether it sits under `[post_model_sync]`.
    pub post_model_sync: bool,
}

/// Read a `patches.txt`.
///
/// Line-oriented, `#` comments, section markers in brackets. `execute:` lines
/// are opaque — they are Python, not a module path, and spec 07 says no
/// resolution and no diagnostic.
#[must_use]
pub fn read_patches(source: &str) -> Vec<Patch> {
    let mut found = Vec::new();
    let mut post = false;
    let mut offset = 0;

    for line in source.split_inclusive('\n') {
        let start = offset;
        offset += line.len();

        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            post = trimmed.contains("post_model_sync");
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("execute:") {
            continue;
        }

        // `path #comment` is the documented inline form.
        let path = trimmed.split('#').next().unwrap_or(trimmed).trim();
        if path.is_empty() {
            continue;
        }
        let at = line.find(path).unwrap_or(0);
        found.push(Patch {
            dotted: path.to_string(),
            span: (start + at, start + at + path.len()),
            post_model_sync: post,
        });
    }

    found
}

#[cfg(test)]
mod patch_tests {
    use super::*;

    #[test]
    fn every_documented_line_form_is_read() {
        let source = "[pre_model_sync]\n\
                      # a comment\n\
                      app.patches.v1_0.one\n\
                      execute:frappe.db.sql(\"select 1\")\n\
                      \n\
                      [post_model_sync]\n\
                      app.patches.v1_0.two #idempotent\n";
        let patches = read_patches(source);

        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].dotted, "app.patches.v1_0.one");
        assert!(!patches[0].post_model_sync);
        // The inline comment is not part of the path.
        assert_eq!(patches[1].dotted, "app.patches.v1_0.two");
        assert!(patches[1].post_model_sync);
        // The span covers the path and nothing else.
        assert_eq!(
            &source[patches[1].span.0..patches[1].span.1],
            "app.patches.v1_0.two"
        );
    }
}
