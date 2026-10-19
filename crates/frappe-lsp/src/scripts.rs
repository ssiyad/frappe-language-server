//! The site's Client and Server Scripts as files —
//! [05](../../../docs/specs/05-live-bridge.md).
//!
//! A script lives in the site's database. The server writes each one to a
//! read-only file under the bench's cache directory, so definition can land on
//! it and any editor can open it without an extension. The copy is never read
//! back: editing a script means editing it on the site.

use std::path::{Path, PathBuf};

use frappe_index::scripts::{ScriptKind, SiteScript};
use serde_json::Value;

/// Write one sync's scripts, replacing whatever the last sync wrote for this
/// site, and say where each one went.
pub fn write(directory: &Path, site: &str, client: &[Value], server: &[Value]) -> Vec<SiteScript> {
    let root = directory.join("scripts").join(safe(site));
    clear(&root);

    let mut written = Vec::new();
    for row in client {
        written.extend(one(&root, ScriptKind::Client, row));
    }
    for row in server {
        written.extend(one(&root, ScriptKind::Server, row));
    }
    written
}

fn one(root: &Path, kind: ScriptKind, row: &Value) -> Option<SiteScript> {
    let name = row.get("name")?.as_str()?.to_string();
    let source = row
        .get("script")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (folder, extension, doctype, trigger, enabled) = match kind {
        ScriptKind::Client => (
            "client",
            "js",
            text(row, "dt"),
            text(row, "view"),
            flag(row, "enabled"),
        ),
        ScriptKind::Server => (
            "server",
            "py",
            text(row, "reference_doctype"),
            text(row, "doctype_event")
                .or_else(|| text(row, "api_method"))
                .or_else(|| text(row, "event_frequency")),
            !flag(row, "disabled"),
        ),
    };

    let path = root
        .join(folder)
        .join(format!("{}.{extension}", safe(&name)));
    if let Err(error) = save(&path, source) {
        tracing::warn!(path = %path.display(), %error, "could not write a site script");
        return None;
    }
    Some(SiteScript {
        kind,
        name,
        doctype,
        trigger,
        enabled,
        path,
    })
}

/// Read-only, so an edit fails where the user makes it instead of being lost
/// at the next sync.
fn save(path: &Path, source: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, source)?;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(path, permissions)
}

/// Remove the last sync's copies. Read-only files are made writable first,
/// which some platforms need before they can be removed.
fn clear(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            clear(&path);
            let _ = std::fs::remove_dir(&path);
        } else if let Ok(metadata) = std::fs::metadata(&path) {
            let mut permissions = metadata.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(&path, permissions);
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// A name as one path segment: a site URL or a script name may hold `/`, `:`
/// and spaces.
fn safe(name: &str) -> String {
    let cleaned: String = name
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('.').to_string();
    if cleaned.is_empty() {
        "_".to_string()
    } else {
        cleaned
    }
}

fn text(row: &Value, key: &str) -> Option<String> {
    row.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn flag(row: &Value, key: &str) -> bool {
    row.get(key)
        .and_then(|value| value.as_i64().or_else(|| value.as_bool().map(i64::from)))
        .is_some_and(|value| value != 0)
}

/// Where the scripts of a bench go: its cache directory, resolved against the
/// bench when relative.
#[must_use]
pub fn directory(bench: &Path, cache_dir: &Path) -> PathBuf {
    if cache_dir.is_absolute() {
        cache_dir.to_path_buf()
    } else {
        bench.join(cache_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scripts_are_written_read_only_and_replaced_on_the_next_sync() {
        let directory =
            std::env::temp_dir().join(format!("frappe-lsp-scripts-{}", std::process::id()));
        let client = [
            json!({"name": "Loan: banner", "dt": "Loan", "view": "Form", "enabled": 1,
                             "script": "frappe.ui.form.on(\"Loan\", {});"}),
        ];
        let server = [
            json!({"name": "Loan/notify", "script_type": "DocType Event",
                             "reference_doctype": "Loan", "doctype_event": "After Save",
                             "disabled": 0, "script": "frappe.msgprint('saved')"}),
        ];

        let written = write(&directory, "http://site.localhost:8000", &client, &server);
        assert_eq!(written.len(), 2);
        let banner = &written[0];
        assert_eq!(banner.doctype.as_deref(), Some("Loan"));
        assert!(banner.enabled);
        assert!(
            banner
                .path
                .ends_with("site.localhost_8000/client/Loan__banner.js"),
            "{:?}",
            banner.path
        );
        assert!(
            std::fs::metadata(&banner.path)
                .expect("written")
                .permissions()
                .readonly()
        );
        assert_eq!(written[1].trigger.as_deref(), Some("After Save"));

        // A script deleted on the site is gone after the next sync.
        let again = write(&directory, "http://site.localhost:8000", &client, &[]);
        assert_eq!(again.len(), 1);
        assert!(!written[1].path.exists());

        clear(&directory);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
