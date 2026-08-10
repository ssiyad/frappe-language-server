//! Module-level Python symbols: classes, defs and their decorators.
//!
//! Extraction ([`crate::extract`]) answers "what does this call mean"; this
//! answers "what does this file declare". The index needs the second to build
//! the controller and whitelisted-method registries of
//! [04](../../../docs/specs/04-metadata-index.md), and
//! [02](../../../docs/specs/02-architecture.md) requires it to come from here:
//! a second Python reader elsewhere would eventually disagree with this one
//! about what a decorator is, and the disagreement would surface as a
//! diagnostic on correct code.

use tree_sitter::{Node, Tree};

use crate::language::{GrammarError, Language};
use crate::matches::Span;

/// Decorators that provably do not whitelist. Anything outside this list and
/// outside the whitelist forms leaves the function [`Whitelist::Unknown`]
/// ([07](../../../docs/specs/07-dotted-paths.md)).
const TRANSPARENT: [&str; 12] = [
    "staticmethod",
    "classmethod",
    "property",
    "abstractmethod",
    "abc.abstractmethod",
    "cached_property",
    "functools.cached_property",
    "functools.lru_cache",
    "functools.wraps",
    "typing.overload",
    "override",
    "typing.override",
];

/// What a Python file declares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Module {
    /// Top-level `class` statements.
    pub classes: Vec<Class>,
    /// Top-level `def` statements.
    pub functions: Vec<Function>,
    /// Local names that mean `frappe.whitelist`, aliases included.
    pub whitelist_names: Vec<String>,
}

impl Module {
    /// A top-level class by name.
    #[must_use]
    pub fn class(&self, name: &str) -> Option<&Class> {
        self.classes.iter().find(|class| class.name == name)
    }

    /// A top-level function by name.
    #[must_use]
    pub fn function(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|function| function.name == name)
    }
}

/// A `class` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    /// The class name.
    pub name: String,
    /// The name alone, for `targetSelectionRange`.
    pub name_span: Span,
    /// The statement and its body.
    pub span: Span,
    /// Base classes as written, `frappe.model.document.Document` included.
    pub bases: Vec<String>,
    /// Methods declared directly in the body.
    pub methods: Vec<Function>,
}

impl Class {
    /// A method by name.
    #[must_use]
    pub fn method(&self, name: &str) -> Option<&Function> {
        self.methods.iter().find(|method| method.name == name)
    }
}

/// A `def` statement, at module level or in a class body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// The function name.
    pub name: String,
    /// The name alone, for `targetSelectionRange`.
    pub name_span: Span,
    /// The statement and its body, decorators included.
    pub span: Span,
    /// Parameter names, in order, without annotations or defaults.
    pub parameters: Vec<String>,
    /// Decorators as written, `@` stripped.
    pub decorators: Vec<String>,
    /// Whether `frappe.call` may reach it.
    pub whitelist: Whitelist,
}

/// Whether a function is exposed by `@frappe.whitelist()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Whitelist {
    /// Undecorated, or decorated only by decorators known not to whitelist.
    No,
    /// Decorated with `frappe.whitelist(...)`, or a `whitelist` imported from
    /// `frappe` under any name.
    Yes(Exposure),
    /// Decorated with something this reader cannot resolve. Never reported as
    /// "not whitelisted" ([07](../../../docs/specs/07-dotted-paths.md)) — a
    /// wrong "not whitelisted" is a false `non-whitelisted-call` error on code
    /// that runs.
    Unknown,
}

/// The arguments `frappe.whitelist()` was called with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exposure {
    /// `allow_guest=True`.
    pub allow_guest: bool,
    /// `methods=[…]`, empty when unspecified.
    pub methods: Vec<String>,
    /// `xss_safe=True`.
    pub xss_safe: bool,
}

/// Read a Python source file.
///
/// # Errors
///
/// Only when the Python grammar cannot be loaded, which is a build problem
/// rather than a source one. A file the parser cannot make sense of still
/// yields a [`Module`] — with fewer symbols, since a missing symbol is a
/// missing completion and a dead session is not.
pub fn read(source: &str) -> Result<Module, GrammarError> {
    let mut parser = Language::Python.parser()?;
    let Some(tree) = parser.parse(source, None) else {
        return Ok(Module::default());
    };
    Ok(read_tree(&tree, source))
}

/// Read from a tree already parsed — an open buffer, typically.
#[must_use]
pub fn read_tree(tree: &Tree, source: &str) -> Module {
    let mut module = Module::default();
    let root = tree.root_node();
    let mut cursor = root.walk();

    // `whitelist` may be imported before or after its use, so imports are
    // collected in a first pass over the same child list.
    for child in root.children(&mut cursor) {
        if child.kind() == "import_from_statement" {
            collect_whitelist_aliases(child, source, &mut module.whitelist_names);
        }
    }

    for child in root.children(&mut cursor) {
        let (definition, decorators) = undecorate(child, source);
        match definition.kind() {
            "class_definition" => {
                if let Some(class) = read_class(definition, child, source, &module.whitelist_names)
                {
                    module.classes.push(class);
                }
            }
            "function_definition" => {
                if let Some(function) = read_function(
                    definition,
                    child,
                    decorators,
                    source,
                    &module.whitelist_names,
                ) {
                    module.functions.push(function);
                }
            }
            _ => {}
        }
    }

    module
}

/// A decorated definition wraps the `def` or `class`; an undecorated one is
/// itself. Either way the outer node carries the span, so decorators count as
/// part of the symbol for navigation.
fn undecorate<'tree>(node: Node<'tree>, source: &str) -> (Node<'tree>, Vec<String>) {
    if node.kind() != "decorated_definition" {
        return (node, Vec::new());
    }

    let mut decorators = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "decorator" {
            decorators.push(decorator_text(child, source));
        }
    }

    let mut fallback = node.walk();
    let definition = node
        .child_by_field_name("definition")
        .or_else(|| {
            node.named_children(&mut fallback)
                .find(|child| matches!(child.kind(), "class_definition" | "function_definition"))
        })
        .unwrap_or(node);
    (definition, decorators)
}

fn read_class(
    definition: Node<'_>,
    outer: Node<'_>,
    source: &str,
    whitelist_names: &[String],
) -> Option<Class> {
    let name = definition.child_by_field_name("name")?;

    let mut bases = Vec::new();
    if let Some(superclasses) = definition.child_by_field_name("superclasses") {
        let mut cursor = superclasses.walk();
        for child in superclasses.named_children(&mut cursor) {
            // A keyword argument is `metaclass=…`, not a base.
            if child.kind() != "keyword_argument" {
                bases.push(text(child, source));
            }
        }
    }

    let mut methods = Vec::new();
    if let Some(body) = definition.child_by_field_name("body") {
        let mut cursor = body.walk();
        for statement in body.children(&mut cursor) {
            let (inner, decorators) = undecorate(statement, source);
            if inner.kind() == "function_definition"
                && let Some(method) =
                    read_function(inner, statement, decorators, source, whitelist_names)
            {
                methods.push(method);
            }
        }
    }

    Some(Class {
        name: text(name, source),
        name_span: span(name),
        span: span(outer),
        bases,
        methods,
    })
}

fn read_function(
    definition: Node<'_>,
    outer: Node<'_>,
    decorators: Vec<String>,
    source: &str,
    whitelist_names: &[String],
) -> Option<Function> {
    let name = definition.child_by_field_name("name")?;

    let mut parameters = Vec::new();
    if let Some(list) = definition.child_by_field_name("parameters") {
        let mut cursor = list.walk();
        for parameter in list.named_children(&mut cursor) {
            if let Some(name) = parameter_name(parameter, source) {
                parameters.push(name);
            }
        }
    }

    let whitelist = classify(&decorators, definition, source, whitelist_names);

    Some(Function {
        name: text(name, source),
        name_span: span(name),
        span: span(outer),
        parameters,
        decorators,
        whitelist,
    })
}

/// `x`, `x: int`, `x = 1`, `*args`, `**kwargs` — the name in each.
fn parameter_name(parameter: Node<'_>, source: &str) -> Option<String> {
    match parameter.kind() {
        "identifier" => Some(text(parameter, source)),
        "typed_parameter" | "list_splat_pattern" | "dictionary_splat_pattern" => parameter
            .named_children(&mut parameter.walk())
            .find(|child| child.kind() == "identifier")
            .map(|child| text(child, source)),
        "default_parameter" | "typed_default_parameter" => parameter
            .child_by_field_name("name")
            .map(|name| text(name, source)),
        // `/` and `*` separators carry no name.
        _ => None,
    }
}

/// Whether the decorators expose this function, and how.
///
/// The tri-state is the point: anything this reader cannot resolve is
/// [`Whitelist::Unknown`], so a decorator factory behind indirection can never
/// produce a false "not whitelisted".
fn classify(
    decorators: &[String],
    definition: Node<'_>,
    source: &str,
    whitelist_names: &[String],
) -> Whitelist {
    if decorators.is_empty() {
        return Whitelist::No;
    }

    let mut unresolved = false;
    for (index, decorator) in decorators.iter().enumerate() {
        let head = decorator
            .split(['(', '['])
            .next()
            .unwrap_or(decorator)
            .trim();
        if head == "frappe.whitelist" || whitelist_names.iter().any(|name| name == head) {
            return Whitelist::Yes(exposure(definition, index, source));
        }
        if !TRANSPARENT.contains(&head) {
            unresolved = true;
        }
    }

    if unresolved {
        Whitelist::Unknown
    } else {
        Whitelist::No
    }
}

/// The keyword arguments of the whitelisting decorator at `index`.
fn exposure(definition: Node<'_>, index: usize, source: &str) -> Exposure {
    let mut exposure = Exposure::default();

    let Some(parent) = definition.parent() else {
        return exposure;
    };
    let mut cursor = parent.walk();
    let Some(decorator) = parent
        .children(&mut cursor)
        .filter(|child| child.kind() == "decorator")
        .nth(index)
    else {
        return exposure;
    };

    // `@frappe.whitelist` without parentheses exposes with every default.
    let Some(call) = decorator
        .named_children(&mut decorator.walk())
        .find(|child| child.kind() == "call")
    else {
        return exposure;
    };
    let Some(arguments) = call.child_by_field_name("arguments") else {
        return exposure;
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
        match text(name, source).as_str() {
            "allow_guest" => exposure.allow_guest = text(value, source) == "True",
            "xss_safe" => exposure.xss_safe = text(value, source) == "True",
            "methods" => {
                let mut cursor = value.walk();
                for item in value.named_children(&mut cursor) {
                    if item.kind() == "string"
                        && let Some(literal) = string_text(item, source)
                    {
                        exposure.methods.push(literal);
                    }
                }
            }
            _ => {}
        }
    }

    exposure
}

/// `from frappe import whitelist as w` — every local name meaning
/// `frappe.whitelist` ([07](../../../docs/specs/07-dotted-paths.md)).
fn collect_whitelist_aliases(statement: Node<'_>, source: &str, names: &mut Vec<String>) {
    let Some(module) = statement.child_by_field_name("module_name") else {
        return;
    };
    if text(module, source) != "frappe" {
        return;
    }

    let mut cursor = statement.walk();
    for child in statement.named_children(&mut cursor) {
        match child.kind() {
            "dotted_name" if text(child, source) == "whitelist" => {
                names.push("whitelist".to_string());
            }
            "aliased_import" => {
                let (Some(name), Some(alias)) = (
                    child.child_by_field_name("name"),
                    child.child_by_field_name("alias"),
                ) else {
                    continue;
                };
                if text(name, source) == "whitelist" {
                    names.push(text(alias, source));
                }
            }
            _ => {}
        }
    }
}

fn decorator_text(decorator: Node<'_>, source: &str) -> String {
    let inner = decorator
        .named_children(&mut decorator.walk())
        .next()
        .unwrap_or(decorator);
    text(inner, source)
}

fn string_text(node: Node<'_>, source: &str) -> Option<String> {
    let raw = text(node, source);
    let trimmed = raw.trim_start_matches(['r', 'b', 'f', 'u', 'R', 'B', 'F', 'U']);
    let quote = trimmed.chars().next()?;
    Some(trimmed.trim_matches(quote).to_string())
}

fn text(node: Node<'_>, source: &str) -> String {
    source
        .get(node.byte_range())
        .unwrap_or_default()
        .to_string()
}

fn span(node: Node<'_>) -> Span {
    let range = node.byte_range();
    Span::new(range.start, range.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(source: &str) -> Module {
        read(source).expect("the Python grammar loads")
    }

    #[test]
    fn a_controller_class_carries_its_bases_and_methods() {
        let module = module(
            "import frappe\n\
             from frappe.model.document import Document\n\
             \n\
             \n\
             class LibraryMember(Document):\n\
             \tdef validate(self):\n\
             \t\tpass\n\
             \n\
             \tdef add_loan(self, title, due_date=None):\n\
             \t\tpass\n",
        );

        let class = module.class("LibraryMember").expect("the class");
        assert_eq!(class.bases, ["Document"]);
        assert_eq!(class.methods.len(), 2);
        assert_eq!(
            class.method("add_loan").expect("method").parameters,
            ["self", "title", "due_date"]
        );
    }

    #[test]
    fn whitelist_is_detected_in_every_documented_spelling() {
        let module = module(
            "import frappe\n\
             from frappe import whitelist\n\
             from frappe import whitelist as w\n\
             \n\
             \n\
             @frappe.whitelist()\n\
             def one():\n\
             \tpass\n\
             \n\
             @whitelist()\n\
             def two():\n\
             \tpass\n\
             \n\
             @w()\n\
             def three():\n\
             \tpass\n\
             \n\
             @frappe.whitelist\n\
             def four():\n\
             \tpass\n",
        );

        for name in ["one", "two", "three", "four"] {
            let function = module.function(name).expect(name);
            assert!(
                matches!(function.whitelist, Whitelist::Yes(_)),
                "{name}: {:?}",
                function.whitelist
            );
        }
    }

    #[test]
    fn exposure_arguments_are_read() {
        let module = module(
            "import frappe\n\
             \n\
             \n\
             @frappe.whitelist(allow_guest=True, methods=[\"GET\", \"POST\"], xss_safe=True)\n\
             def public():\n\
             \tpass\n",
        );

        let Whitelist::Yes(exposure) = &module.function("public").expect("public").whitelist else {
            panic!("expected a whitelisted function");
        };
        assert!(exposure.allow_guest);
        assert!(exposure.xss_safe);
        assert_eq!(exposure.methods, ["GET", "POST"]);
    }

    #[test]
    fn an_undecorated_function_is_not_whitelisted() {
        let module = module("def helper():\n\tpass\n");
        assert_eq!(
            module.function("helper").expect("helper").whitelist,
            Whitelist::No
        );
    }

    #[test]
    fn a_transparent_decorator_does_not_hide_the_answer() {
        let module = module(
            "import frappe\n\
             \n\
             \n\
             class Thing:\n\
             \t@staticmethod\n\
             \tdef plain():\n\
             \t\tpass\n\
             \n\
             \t@staticmethod\n\
             \t@frappe.whitelist()\n\
             \tdef exposed():\n\
             \t\tpass\n",
        );

        let class = module.class("Thing").expect("Thing");
        assert_eq!(
            class.method("plain").expect("plain").whitelist,
            Whitelist::No
        );
        assert!(matches!(
            class.method("exposed").expect("exposed").whitelist,
            Whitelist::Yes(_)
        ));
    }

    #[test]
    fn an_unresolvable_decorator_yields_unknown_never_no() {
        let module = module(
            "from mystery import guard\n\
             \n\
             \n\
             @guard\n\
             def maybe_exposed():\n\
             \tpass\n",
        );

        // Reporting this as "not whitelisted" would be a false
        // non-whitelisted-call error on code that runs — specs/07.
        assert_eq!(
            module
                .function("maybe_exposed")
                .expect("function")
                .whitelist,
            Whitelist::Unknown
        );
    }

    #[test]
    fn a_docstring_cannot_fake_a_definition() {
        let module = module(
            "def real():\n\
             \t\"\"\"\n\
             \tclass Fake(Document):\n\
             \t\tdef method(self):\n\
             \t\t\tpass\n\
             \t\"\"\"\n",
        );

        assert!(module.classes.is_empty());
        assert_eq!(module.functions.len(), 1);
    }

    #[test]
    fn a_nested_definition_is_not_a_module_symbol() {
        let module = module(
            "def outer():\n\
             \tdef inner():\n\
             \t\tpass\n\
             \n\
             \tclass Local:\n\
             \t\tpass\n",
        );

        assert_eq!(module.functions.len(), 1);
        assert!(module.classes.is_empty());
    }

    #[test]
    fn a_syntax_error_still_yields_the_symbols_around_it() {
        // A half-typed buffer is the common case, not the exceptional one.
        let module = module(
            "class Good:\n\
             \tdef fine(self):\n\
             \t\tpass\n\
             \n\
             \n\
             def broken(\n",
        );

        assert!(module.class("Good").is_some());
    }

    #[test]
    fn spans_point_at_the_name_and_the_whole_statement() {
        let source = "@frappe.whitelist()\ndef ping():\n\tpass\n";
        let module = module(source);
        let function = module.function("ping").expect("ping");

        assert_eq!(
            &source[function.name_span.start..function.name_span.end],
            "ping"
        );
        // The span covers the decorator: navigation should land above it.
        assert!(source[function.span.start..function.span.end].starts_with("@frappe.whitelist()"));
    }
}
