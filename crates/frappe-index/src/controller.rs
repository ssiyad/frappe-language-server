//! Controller resolution — [04](../../../docs/specs/04-metadata-index.md).
//!
//! `<x>.py` beside the JSON is the base controller. `override_doctype_class`
//! and `extend_doctype_class` alter it; both are hook values, extracted by 010,
//! and land here through [`ControllerRef::overrides`] once that exists.

use std::path::{Path, PathBuf};

use crate::python::{self, Whitelist};
use crate::span::Range;

/// The controller class of a DocType.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerRef {
    /// `<x>.py`, beside the DocType JSON.
    pub path: PathBuf,
    /// The dotted path of the module ([07](../../../docs/specs/07-dotted-paths.md)).
    pub module: String,
    /// The class name.
    pub class_name: String,
    /// The class statement and its body.
    pub range: Range,
    /// The class name alone.
    pub name_range: Range,
    /// Methods declared on it.
    pub methods: Vec<Method>,
}

impl ControllerRef {
    /// The dotted path of the class itself.
    #[must_use]
    pub fn dotted(&self) -> String {
        format!("{}.{}", self.module, self.class_name)
    }

    /// A method by name.
    #[must_use]
    pub fn method(&self, name: &str) -> Option<&Method> {
        self.methods.iter().find(|method| method.name == name)
    }

    /// Classes that replace or extend this one. Empty until 010 extracts
    /// `override_doctype_class` and `extend_doctype_class` from `hooks.py`.
    #[must_use]
    pub fn overrides(&self) -> &[ControllerRef] {
        &[]
    }
}

/// A method on a controller class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    /// The method name.
    pub name: String,
    /// The `def` and its body, decorators included.
    pub range: Range,
    /// The name alone.
    pub name_range: Range,
    /// Parameters, `self` included.
    pub parameters: Vec<String>,
    /// Whether `@frappe.whitelist()` exposes it.
    pub whitelisted: bool,
}

/// Pick the controller class out of a parsed `<x>.py`.
///
/// Frappe files hold more than one class often enough — mixins, exceptions,
/// helpers — that "the first class" is wrong. In order: a class deriving from
/// something named `Document`, then the class named after the DocType, then the
/// only class in the file. A file with several unrelated classes and no
/// `Document` base yields nothing rather than a guess.
#[must_use]
pub fn resolve(
    module: &python::Module,
    path: &Path,
    dotted: &str,
    doctype: &str,
) -> Option<ControllerRef> {
    let expected: String = doctype
        .split([' ', '-', '_'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut characters = word.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                None => String::new(),
            }
        })
        .collect();

    let class = module
        .classes
        .iter()
        .find(|class| {
            class
                .bases
                .iter()
                .any(|base| base == "Document" || base.ends_with(".Document"))
        })
        .or(module.class(&expected))
        .or(match module.classes.as_slice() {
            [only] => Some(only),
            _ => None,
        })?;

    Some(ControllerRef {
        path: path.to_path_buf(),
        module: dotted.to_string(),
        class_name: class.name.clone(),
        range: class.range,
        name_range: class.name_range,
        methods: class
            .methods
            .iter()
            .map(|method| Method {
                name: method.name.clone(),
                range: method.range,
                name_range: method.name_range,
                parameters: method.parameters.clone(),
                whitelisted: matches!(method.whitelist, Whitelist::Yes(_)),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_document_subclass_wins_over_the_helpers() {
        let module = python::read(
            "\
class Helper:
\tpass


class LibraryMember(Document):
\tdef validate(self):
\t\tpass
",
        );
        let controller = resolve(&module, Path::new("x.py"), "library.x", "Library Member")
            .expect("a controller");

        assert_eq!(controller.class_name, "LibraryMember");
        assert_eq!(controller.dotted(), "library.x.LibraryMember");
        assert_eq!(controller.methods.len(), 1);
    }

    #[test]
    fn a_qualified_document_base_still_counts() {
        let module = python::read("class Loan(frappe.model.document.Document):\n\tpass\n");
        let controller =
            resolve(&module, Path::new("x.py"), "circulation.x", "Loan").expect("a controller");
        assert_eq!(controller.class_name, "Loan");
    }

    #[test]
    fn the_camel_case_name_is_the_fallback() {
        let module = python::read("class LibraryLoanItem(BaseThing):\n\tpass\n");
        let controller = resolve(&module, Path::new("x.py"), "library.x", "Library Loan Item")
            .expect("a controller");
        assert_eq!(controller.class_name, "LibraryLoanItem");
    }

    #[test]
    fn a_file_with_no_plausible_class_yields_nothing() {
        let module = python::read("class Alpha:\n\tpass\n\n\nclass Beta:\n\tpass\n");
        assert_eq!(
            resolve(&module, Path::new("x.py"), "library.x", "Gamma"),
            None
        );
        assert_eq!(
            resolve(
                &python::read("def helper():\n\tpass\n"),
                Path::new("x.py"),
                "library.x",
                "Gamma"
            ),
            None
        );
    }

    #[test]
    fn a_whitelisted_method_is_marked() {
        let module = python::read(
            "\
import frappe


class LibraryMember(Document):
\t@frappe.whitelist()
\tdef renew_all(self):
\t\tpass

\tdef quiet(self):
\t\tpass
",
        );
        let controller = resolve(&module, Path::new("x.py"), "library.x", "Library Member")
            .expect("a controller");

        assert!(
            controller
                .method("renew_all")
                .expect("renew_all")
                .whitelisted
        );
        assert!(!controller.method("quiet").expect("quiet").whitelisted);
    }
}
