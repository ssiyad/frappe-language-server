//! What a Jinja template can name without importing anything —
//! [13](../../../docs/specs/13-surface-jinja.md).
//!
//! frappe builds the template globals in code: nested `NamespaceDict(name=…)`
//! calls in `frappe/utils/safe_exec.py`, plus whatever the `jinja` hook adds.
//! The names are read from the bench's own frappe, so they match the version
//! installed rather than a list kept by hand. They feed completion only.

use std::collections::{BTreeMap, BTreeSet};

use frappe_syntax::Language;

/// The functions whose `NamespaceDict` calls make up the globals: the
/// restricted set, and what unrestricted rendering adds to it.
const BUILDERS: [&str; 2] = ["render_safe_globals", "exec_safe_globals"];

/// Global names by namespace: `""` for the top level, `"frappe"` for
/// `frappe.<name>`, `"frappe.db"` for `frappe.db.<name>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JinjaGlobals {
    namespaces: BTreeMap<String, BTreeSet<String>>,
}

impl JinjaGlobals {
    /// The names under a namespace, `""` for the top level.
    pub fn members(&self, namespace: &str) -> impl Iterator<Item = &str> {
        self.namespaces
            .get(namespace)
            .into_iter()
            .flatten()
            .map(String::as_str)
    }

    /// Whether `name` under `namespace` is itself a namespace.
    #[must_use]
    pub fn is_namespace(&self, namespace: &str, name: &str) -> bool {
        let full = if namespace.is_empty() {
            name.to_string()
        } else {
            format!("{namespace}.{name}")
        };
        self.namespaces.contains_key(&full)
    }

    /// Add one top-level name, as the `jinja` hook does.
    pub fn add(&mut self, name: &str) {
        self.namespaces
            .entry(String::new())
            .or_default()
            .insert(name.to_string());
    }

    /// Read the globals out of `frappe/utils/safe_exec.py`.
    #[must_use]
    pub fn scrape(safe_exec: &str) -> Self {
        let mut globals = Self::default();
        let Ok(mut parser) = Language::Python.parser() else {
            return globals;
        };
        let Some(tree) = parser.parse(safe_exec, None) else {
            return globals;
        };

        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            let mut cursor = node.walk();
            stack.extend(node.named_children(&mut cursor));
            let is_builder = node.kind() == "function_definition"
                && node
                    .child_by_field_name("name")
                    .is_some_and(|name| BUILDERS.contains(&&safe_exec[name.byte_range()]));
            if !is_builder {
                continue;
            }
            // The first `NamespaceDict(...)` a builder assigns is its top level.
            let Some(body) = node.child_by_field_name("body") else {
                continue;
            };
            if let Some(top) = first_namespace(body, safe_exec) {
                globals.collect(top, safe_exec, "");
            }
        }
        globals
    }

    /// The keyword names of one `NamespaceDict(...)` call, recursing into the
    /// ones whose value is a `NamespaceDict` too.
    fn collect(&mut self, call: tree_sitter::Node<'_>, source: &str, namespace: &str) {
        let Some(arguments) = call.child_by_field_name("arguments") else {
            return;
        };
        let mut cursor = arguments.walk();
        for argument in arguments.named_children(&mut cursor) {
            if argument.kind() != "keyword_argument" {
                continue;
            }
            let (Some(name), Some(value)) = (
                argument.child_by_field_name("name"),
                argument.child_by_field_name("value"),
            ) else {
                continue;
            };
            let name = &source[name.byte_range()];
            self.namespaces
                .entry(namespace.to_string())
                .or_default()
                .insert(name.to_string());
            if is_namespace_call(value, source) {
                let inner = if namespace.is_empty() {
                    name.to_string()
                } else {
                    format!("{namespace}.{name}")
                };
                self.collect(value, source, &inner);
            }
        }
    }
}

fn is_namespace_call(node: tree_sitter::Node<'_>, source: &str) -> bool {
    node.kind() == "call"
        && node
            .child_by_field_name("function")
            .is_some_and(|function| &source[function.byte_range()] == "NamespaceDict")
}

/// The first `NamespaceDict(...)` call in source order under `node`.
fn first_namespace<'t>(node: tree_sitter::Node<'t>, source: &str) -> Option<tree_sitter::Node<'t>> {
    if is_namespace_call(node, source) {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    children
        .into_iter()
        .find_map(|child| first_namespace(child, source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nested_namespaces_are_read() {
        let source = r#"
def render_safe_globals():
	out = NamespaceDict(
		json=NamespaceDict(loads=json.loads, dumps=json.dumps),
		frappe=NamespaceDict(
			format=frappe.format_value,
			db=NamespaceDict(get_value=safe_get_value, exists=frappe.db.exists),
		),
		_=frappe._,
	)
	return out

def exec_safe_globals():
	exec_safe = NamespaceDict(frappe=NamespaceDict(sendmail=frappe.sendmail))
	return exec_safe

def unrelated():
	return NamespaceDict(secret=1)
"#;
        let globals = JinjaGlobals::scrape(source);
        let top: Vec<&str> = globals.members("").collect();
        assert_eq!(top, ["_", "frappe", "json"]);
        let frappe: Vec<&str> = globals.members("frappe").collect();
        assert_eq!(frappe, ["db", "format", "sendmail"]);
        assert!(globals.members("frappe.db").any(|name| name == "get_value"));
        assert!(globals.is_namespace("frappe", "db"));
        assert!(!globals.members("").any(|name| name == "secret"));
    }
}
