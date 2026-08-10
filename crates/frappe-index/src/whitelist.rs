//! The whitelisted-method registry of
//! [07](../../../docs/specs/07-dotted-paths.md).
//!
//! `frappe.call` from JavaScript reaches exactly these, so a call to anything
//! else is a guaranteed runtime `PermissionError` — the flagship check of
//! [23](../../../docs/specs/23-diagnostics.md).

use std::path::{Path, PathBuf};

use crate::python::{self, Whitelist};
use crate::span::Range;

/// One `@frappe.whitelist()` target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhitelistedMethod {
    /// The dotted path `frappe.call` names.
    pub dotted: String,
    /// The file holding it.
    pub path: PathBuf,
    /// `allow_guest=True`.
    pub allow_guest: bool,
    /// `methods=[…]`; empty means Frappe's default, every method.
    pub methods: Vec<String>,
    /// Parameter names, in order — what a `frappe.call` `args` map may hold.
    pub parameters: Vec<String>,
    /// The definition and its body.
    pub range: Range,
    /// The name alone.
    pub name_range: Range,
    /// The class, for a whitelisted controller method.
    pub owner: Option<String>,
}

/// Every whitelisted method a parsed module declares.
///
/// Controller methods are registered as `<module>.<Class>.<method>`. Frappe
/// itself dispatches those through `run_doc_method` by bare name, but the
/// dotted form is the only spelling that resolves back to a location, and
/// [07](../../../docs/specs/07-dotted-paths.md) makes the dotted path the key.
#[must_use]
pub fn methods_of(module: &python::Module, path: &Path, dotted: &str) -> Vec<WhitelistedMethod> {
    let mut found = Vec::new();

    for function in &module.functions {
        let Whitelist::Yes(exposure) = &function.whitelist else {
            continue;
        };
        found.push(WhitelistedMethod {
            dotted: format!("{dotted}.{}", function.name),
            path: path.to_path_buf(),
            allow_guest: exposure.allow_guest,
            methods: exposure.methods.clone(),
            parameters: function.parameters.clone(),
            range: function.range,
            name_range: function.name_range,
            owner: None,
        });
    }

    for class in &module.classes {
        for method in &class.methods {
            let Whitelist::Yes(exposure) = &method.whitelist else {
                continue;
            };
            found.push(WhitelistedMethod {
                dotted: format!("{dotted}.{}.{}", class.name, method.name),
                path: path.to_path_buf(),
                allow_guest: exposure.allow_guest,
                methods: exposure.methods.clone(),
                parameters: method.parameters.clone(),
                range: method.range,
                name_range: method.name_range,
                owner: Some(class.name.clone()),
            });
        }
    }

    found.sort_by(|a, b| a.dotted.cmp(&b.dotted));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_functions_and_controller_methods_both_register() {
        let module = python::read(
            "\
import frappe
from frappe import whitelist


@frappe.whitelist(allow_guest=True, methods=[\"GET\"])
def list_issued_titles():
\tpass


@whitelist()
def ping():
\tpass


def quiet():
\tpass


class LibraryMember(Document):
\t@frappe.whitelist()
\tdef renew_all(self, count: int = 1):
\t\tpass
",
        );

        let found = methods_of(
            &module,
            Path::new("/apps/library/library/api.py"),
            "library.api",
        );
        let paths: Vec<&str> = found.iter().map(|method| method.dotted.as_str()).collect();
        assert_eq!(
            paths,
            [
                "library.api.LibraryMember.renew_all",
                "library.api.list_issued_titles",
                "library.api.ping",
            ],
            "`quiet` is not whitelisted and must not appear"
        );

        let listing = &found[1];
        assert!(listing.allow_guest);
        assert_eq!(listing.methods, ["GET"]);

        let renew = &found[0];
        assert_eq!(renew.owner.as_deref(), Some("LibraryMember"));
        assert_eq!(renew.parameters, ["self", "count"]);
        assert!(!renew.allow_guest);
    }

    #[test]
    fn a_module_with_nothing_whitelisted_registers_nothing() {
        let module = python::read("def helper():\n\tpass\n");
        assert!(methods_of(&module, Path::new("/x.py"), "library.x").is_empty());
    }
}
