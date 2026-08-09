//! Credential detection.
//!
//! Spec 30 forbids credentials in any configuration source and requires a file
//! carrying one to be rejected rather than silently used: committed credentials
//! are the failure mode worth designing against, and a config that half-works
//! is how one survives review.

use serde_json::Value;

/// Key name fragments, matched after stripping separators and lowercasing.
/// `bridge.profile` names a profile, never a secret, so `profile` is absent.
const SECRET_FRAGMENTS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "apikey",
    "credential",
    "privatekey",
    "cookie",
];

/// Whole key names too short to match as fragments — `sid` is Frappe's session
/// cookie, and `outside` contains it.
const SECRET_NAMES: &[&str] = &["sid", "pwd"];

/// The dotted path of the first key that looks like a credential.
pub fn find(value: &Value) -> Option<String> {
    walk(value, &mut Vec::new())
}

fn walk(value: &Value, path: &mut Vec<String>) -> Option<String> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if is_secret_key(key) {
                    path.push(key.clone());
                    return Some(path.join("."));
                }
                path.push(key.clone());
                if let Some(found) = walk(child, path) {
                    return Some(found);
                }
                path.pop();
            }
            None
        }
        Value::Array(items) => {
            for (position, child) in items.iter().enumerate() {
                path.push(position.to_string());
                if let Some(found) = walk(child, path) {
                    return Some(found);
                }
                path.pop();
            }
            None
        }
        Value::String(text) if looks_like_secret_value(text) => Some(path.join(".")),
        _ => None,
    }
}

fn is_secret_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    SECRET_FRAGMENTS
        .iter()
        .any(|needle| normalized.contains(needle))
        || SECRET_NAMES.contains(&normalized.as_str())
}

fn looks_like_secret_value(value: &str) -> bool {
    // `Authorization: token <api_key>:<api_secret>`, the header Frappe's REST
    // API takes — the shape a user copies out of a working curl invocation.
    if let Some(rest) = value.strip_prefix("token ")
        && let Some((key, secret)) = rest.split_once(':')
        && !key.trim().is_empty()
        && !secret.trim().is_empty()
    {
        return true;
    }

    // A long run of bare alphanumerics is a generated hash, not a site name, a
    // path or a glob — all of which carry `.`, `/`, `-` or `*`.
    value.len() >= 24 && value.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toml_value(source: &str) -> Value {
        toml::from_str(source).expect("valid TOML")
    }

    #[test]
    fn secret_key_names_are_caught() {
        for source in [
            "[bridge]\napi_secret = \"x\"\n",
            "[bridge]\napi_key = \"x\"\n",
            "[bridge]\npassword = \"x\"\n",
            "[bridge]\nauthToken = \"x\"\n",
        ] {
            assert!(find(&toml_value(source)).is_some(), "missed: {source}");
        }
    }

    #[test]
    fn secret_shaped_values_are_caught() {
        let found = find(&toml_value(
            "[bridge]\nsite = \"token 8a1c9f0e2b:5d3f7a1c9e0b\"\n",
        ));
        assert_eq!(found.as_deref(), Some("bridge.site"));

        let found = find(&toml_value(
            "[bridge]\nsite = \"f2c1a9e0b7d43a615c8e02fb\"\n",
        ));
        assert_eq!(found.as_deref(), Some("bridge.site"));
    }

    #[test]
    fn the_documented_schema_is_clean() {
        let schema = r#"
[workspace]
root = "auto"
apps = []
exclude = ["**/node_modules/**"]

[index]
cache = true
cache_dir = ".frappe-lsp"

[bridge]
enabled = false
site = "erp.example.localhost"
profile = "production"
refresh_interval = 0

[diagnostics]
enabled = true
unknown-doctype = "warning"

[types]
stub = false
stub_dir = ".frappe-lsp/stubs"
scope = "referenced"
cast_action = true
"#;
        assert_eq!(find(&toml_value(schema)), None);
    }
}
