//! A structural reader for Python source: classes, defs and decorators.
//!
//! Deliberately not a parse. `frappe-syntax` owns tree-sitter and is a sibling
//! crate this one must not depend on
//! ([02](../../../docs/specs/02-architecture.md)), so the controller and
//! whitelist registries of [04](../../../docs/specs/04-metadata-index.md) are
//! read with the same line-oriented technique
//! [`crate::app::python_string`] already uses for `app_name`.
//!
//! What that buys and costs: `def`, `class` and `@decorator` are unambiguous at
//! the start of a logical line, and string and comment text is masked out before
//! anything is matched, so the shapes below cannot be faked by a docstring. What
//! it cannot see is anything computed — a `def` produced by `exec`, a decorator
//! aliased through a dictionary. Those degrade to
//! [`Whitelist::Unknown`], never to a false "not whitelisted"
//! ([07](../../../docs/specs/07-dotted-paths.md)).

use crate::span::Range;

/// What a Python file declares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Module {
    /// Top-level `class` statements.
    pub classes: Vec<Class>,
    /// Top-level `def` statements.
    pub functions: Vec<Function>,
    /// Local names that mean `frappe.whitelist`, including aliases.
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
    pub name_range: Range,
    /// The statement and its body.
    pub range: Range,
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
    pub name_range: Range,
    /// The statement and its body, decorators included.
    pub range: Range,
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
    /// "not whitelisted" ([07](../../../docs/specs/07-dotted-paths.md)).
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

/// Decorators that provably do not whitelist. Anything outside this list and
/// outside the whitelist forms leaves the function [`Whitelist::Unknown`].
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

/// Read a Python source file.
///
/// Never fails: a file this reader cannot make sense of yields fewer symbols,
/// and a missing symbol is a missing completion, not a dead session.
#[must_use]
pub fn read(source: &str) -> Module {
    let masked = mask(source);
    let mut definitions: Vec<Definition> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut pending: Vec<(Range, String)> = Vec::new();
    // `frappe.whitelist` is unambiguous whatever the import looks like; only
    // the unqualified aliases have to be learned from an import statement.
    let mut whitelist_names = vec!["frappe.whitelist".to_string()];

    let mut at = 0usize;
    while at < masked.len() {
        let end = masked[at..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(masked.len(), |offset| at + offset);
        let line = &masked[at..end];
        let indent = line
            .iter()
            .take_while(|byte| **byte == b' ' || **byte == b'\t')
            .count();
        let body = &line[indent..];
        let start = at + indent;

        // Blankness is judged on the source, not the mask: a comment line
        // between two defs belongs to the second one, and masking it to spaces
        // would otherwise swallow it into the first one's range.
        let raw = source.as_bytes().get(at..end).unwrap_or_default();
        let raw_indent = raw
            .iter()
            .take_while(|byte| **byte == b' ' || **byte == b'\t')
            .count();
        if raw_indent < raw.len() {
            // A statement at or left of an open definition's indent ends it.
            while let Some(last) = open.last() {
                if raw_indent > definitions[*last].indent {
                    break;
                }
                definitions[*last].end = at;
                open.pop();
            }
        }

        if body.starts_with(b"@") {
            let consumed = logical(&masked, start);
            pending.push((
                Range::new(start, consumed),
                decorator_text(source, start, consumed),
            ));
            at = line_after(&masked, consumed);
            continue;
        }

        if let Some(rest) = strip_keyword(body, b"def") {
            let consumed = logical(&masked, start);
            let name_at = start + (line.len() - indent - rest.len());
            let (name, name_range) = identifier(source, &masked, name_at);
            let parameters = parameters(source, &masked, start, consumed);
            let decorators: Vec<String> = pending.iter().map(|(_, text)| text.clone()).collect();
            let definition_start = pending
                .first()
                .map_or(start, |(range, _)| range.start as usize);
            pending.clear();

            definitions.push(Definition {
                kind: Kind::Function,
                name,
                name_range,
                start: definition_start,
                end: masked.len(),
                indent,
                parent: open.last().copied(),
                parameters,
                bases: Vec::new(),
                decorators,
            });
            open.push(definitions.len() - 1);
            at = line_after(&masked, consumed);
            continue;
        }

        if let Some(rest) = strip_keyword(body, b"class") {
            let consumed = logical(&masked, start);
            let name_at = start + (line.len() - indent - rest.len());
            let (name, name_range) = identifier(source, &masked, name_at);
            let bases = parameters(source, &masked, start, consumed);
            let definition_start = pending
                .first()
                .map_or(start, |(range, _)| range.start as usize);
            pending.clear();

            definitions.push(Definition {
                kind: Kind::Class,
                name,
                name_range,
                start: definition_start,
                end: masked.len(),
                indent,
                parent: open.last().copied(),
                parameters: Vec::new(),
                bases,
                decorators: Vec::new(),
            });
            open.push(definitions.len() - 1);
            at = line_after(&masked, consumed);
            continue;
        }

        if !body.is_empty() && !body.starts_with(b"@") {
            pending.clear();
        }
        if body.starts_with(b"from ") {
            let consumed = logical(&masked, start);
            let statement = source.get(start..consumed).unwrap_or_default();
            read_import(statement, &mut whitelist_names);
            at = line_after(&masked, consumed);
            continue;
        }

        at = end + 1;
    }

    whitelist_names.sort();
    whitelist_names.dedup();

    assemble(definitions, whitelist_names)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Class,
    Function,
}

#[derive(Debug, Clone)]
struct Definition {
    kind: Kind,
    name: String,
    name_range: Range,
    start: usize,
    end: usize,
    indent: usize,
    parent: Option<usize>,
    parameters: Vec<String>,
    bases: Vec<String>,
    decorators: Vec<String>,
}

/// Turn the flat definition list into top-level classes and functions.
fn assemble(definitions: Vec<Definition>, whitelist_names: Vec<String>) -> Module {
    let function_of = |definition: &Definition| Function {
        name: definition.name.clone(),
        name_range: definition.name_range,
        range: Range::new(definition.start, definition.end),
        parameters: definition.parameters.clone(),
        decorators: definition.decorators.clone(),
        whitelist: classify(&definition.decorators, &whitelist_names),
    };

    let mut classes = Vec::new();
    let mut functions = Vec::new();

    for (at, definition) in definitions.iter().enumerate() {
        match (definition.kind, definition.parent) {
            (Kind::Class, None) => {
                let methods = definitions
                    .iter()
                    .filter(|child| child.kind == Kind::Function && child.parent == Some(at))
                    .map(function_of)
                    .collect();
                classes.push(Class {
                    name: definition.name.clone(),
                    name_range: definition.name_range,
                    range: Range::new(definition.start, definition.end),
                    bases: definition.bases.clone(),
                    methods,
                });
            }
            (Kind::Function, None) => functions.push(function_of(definition)),
            // Nested classes and closures are not reachable through a dotted
            // path ([07](../../../docs/specs/07-dotted-paths.md)).
            _ => {}
        }
    }

    Module {
        classes,
        functions,
        whitelist_names,
    }
}

/// Decide whether a decorator list whitelists.
fn classify(decorators: &[String], whitelist_names: &[String]) -> Whitelist {
    let mut unresolved = false;
    for decorator in decorators {
        let callee = decorator
            .split_once('(')
            .map_or(decorator.as_str(), |(callee, _)| callee)
            .trim();

        if whitelist_names.iter().any(|name| name == callee) {
            return Whitelist::Yes(exposure(decorator));
        }
        if !TRANSPARENT.contains(&callee) {
            unresolved = true;
        }
    }

    if unresolved {
        Whitelist::Unknown
    } else {
        Whitelist::No
    }
}

/// `allow_guest`, `methods` and `xss_safe` out of a `whitelist(...)` call.
fn exposure(decorator: &str) -> Exposure {
    let Some(arguments) = decorator.split_once('(').map(|(_, rest)| rest) else {
        return Exposure::default();
    };
    let arguments = arguments.strip_suffix(')').unwrap_or(arguments);

    Exposure {
        allow_guest: keyword_is_true(arguments, "allow_guest"),
        xss_safe: keyword_is_true(arguments, "xss_safe"),
        methods: keyword_list(arguments, "methods"),
    }
}

fn keyword_value<'a>(arguments: &'a str, keyword: &str) -> Option<&'a str> {
    let at = arguments.find(&format!("{keyword}="))?;
    Some(&arguments[at + keyword.len() + 1..])
}

fn keyword_is_true(arguments: &str, keyword: &str) -> bool {
    keyword_value(arguments, keyword).is_some_and(|rest| rest.trim_start().starts_with("True"))
}

fn keyword_list(arguments: &str, keyword: &str) -> Vec<String> {
    let Some(rest) = keyword_value(arguments, keyword) else {
        return Vec::new();
    };
    let rest = rest.trim_start();
    let Some(inner) = rest
        .strip_prefix('[')
        .or_else(|| rest.strip_prefix('('))
        .and_then(|inner| inner.split([']', ')']).next())
    else {
        return Vec::new();
    };

    inner
        .split(',')
        .map(|item| item.trim().trim_matches(['"', '\'']).to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// `from frappe import whitelist [as alias]`, the only import that can rename
/// the decorator ([07](../../../docs/specs/07-dotted-paths.md)).
fn read_import(statement: &str, whitelist_names: &mut Vec<String>) {
    let statement = statement.replace(['\n', '\r', '(', ')'], " ");
    let statement = statement.trim();

    let Some(rest) = statement.strip_prefix("from ") else {
        return;
    };
    let Some((module, imported)) = rest.split_once(" import ") else {
        return;
    };
    if module.trim() != "frappe" {
        return;
    }

    for item in imported.split(',') {
        let item = item.trim();
        let (name, alias) = match item.split_once(" as ") {
            Some((name, alias)) => (name.trim(), alias.trim()),
            None => (item, item),
        };
        if name == "whitelist" && !alias.is_empty() {
            whitelist_names.push(alias.to_string());
        }
    }
}

/// The masked source with string and comment content replaced by spaces.
///
/// Line structure survives — newlines are kept even inside triple-quoted
/// strings — so offsets in the mask are offsets in the source.
fn mask(source: &str) -> Vec<u8> {
    let bytes = source.as_bytes();
    let mut masked = bytes.to_vec();
    let mut at = 0;

    while at < bytes.len() {
        match bytes[at] {
            b'#' => {
                while at < bytes.len() && bytes[at] != b'\n' {
                    masked[at] = b' ';
                    at += 1;
                }
            }
            quote @ (b'"' | b'\'') => {
                let triple = bytes[at..].starts_with(&[quote, quote, quote]);
                let width = if triple { 3 } else { 1 };
                blank(&mut masked, at, at + width);
                at += width;

                while at < bytes.len() {
                    if bytes[at] == b'\\' {
                        masked[at] = b' ';
                        if at + 1 < bytes.len() && bytes[at + 1] != b'\n' {
                            masked[at + 1] = b' ';
                        }
                        at += 2;
                        continue;
                    }
                    if !triple && bytes[at] == b'\n' {
                        break;
                    }
                    if triple && bytes[at..].starts_with(&[quote, quote, quote]) {
                        blank(&mut masked, at, at + 3);
                        at += 3;
                        break;
                    }
                    if !triple && bytes[at] == quote {
                        masked[at] = b' ';
                        at += 1;
                        break;
                    }
                    if bytes[at] != b'\n' {
                        masked[at] = b' ';
                    }
                    at += 1;
                }
            }
            _ => at += 1,
        }
    }

    masked
}

/// Replace `masked[from..to]` with spaces, clamped to the buffer.
fn blank(masked: &mut [u8], from: usize, to: usize) {
    let to = to.min(masked.len());
    if from < to {
        masked[from..to].fill(b' ');
    }
}

/// The end offset of the logical line starting at `start`: physical lines are
/// joined while a bracket is open or a backslash continues them.
fn logical(masked: &[u8], start: usize) -> usize {
    let mut depth = 0i32;
    let mut at = start;
    while at < masked.len() {
        match masked[at] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'\\' if masked.get(at + 1) == Some(&b'\n') => {
                at += 2;
                continue;
            }
            b'\n' if depth <= 0 => return at,
            _ => {}
        }
        at += 1;
    }
    masked.len()
}

/// The offset of the first line strictly after `end`.
fn line_after(masked: &[u8], end: usize) -> usize {
    masked[end..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(masked.len(), |offset| end + offset + 1)
}

/// `def` / `class` at the head of a statement, returning what follows it.
fn strip_keyword<'a>(body: &'a [u8], keyword: &[u8]) -> Option<&'a [u8]> {
    let body = body
        .strip_prefix(b"async ".as_slice())
        .map_or(body, |rest| trim_start(rest));
    let rest = body.strip_prefix(keyword)?;
    if !matches!(rest.first(), Some(b' ' | b'\t')) {
        return None;
    }
    Some(trim_start(rest))
}

fn trim_start(bytes: &[u8]) -> &[u8] {
    let at = bytes
        .iter()
        .take_while(|byte| **byte == b' ' || **byte == b'\t')
        .count();
    &bytes[at..]
}

/// The identifier at `at`, read out of the original source.
fn identifier(source: &str, masked: &[u8], at: usize) -> (String, Range) {
    let mut end = at;
    while end < masked.len()
        && (masked[end].is_ascii_alphanumeric() || masked[end] == b'_' || masked[end] >= 0x80)
    {
        end += 1;
    }
    (
        source.get(at..end).unwrap_or_default().to_string(),
        Range::new(at, end),
    )
}

/// Parameter or base-class names between the first `(` and its match.
fn parameters(source: &str, masked: &[u8], start: usize, end: usize) -> Vec<String> {
    let Some(open) = masked[start..end].iter().position(|byte| *byte == b'(') else {
        return Vec::new();
    };
    let open = start + open;
    let mut depth = 0i32;
    let mut close = open;
    for (offset, byte) in masked.iter().enumerate().take(end).skip(open) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    close = offset;
                    break;
                }
            }
            _ => {}
        }
    }
    if close <= open {
        return Vec::new();
    }

    split_top_level(source.get(open + 1..close).unwrap_or_default())
        .into_iter()
        .map(|item| {
            item.split(['=', ':'])
                .next()
                .unwrap_or_default()
                .trim()
                .trim_start_matches('*')
                .trim()
                .to_string()
        })
        .filter(|item| !item.is_empty())
        .collect()
}

/// Split on commas that are not inside brackets — `x: dict[str, int]` is one
/// parameter, not two.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (at, character) in text.char_indices() {
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                items.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    items.push(&text[start..]);
    items
}

/// A decorator as written, `@` stripped and newlines flattened.
fn decorator_text(source: &str, start: usize, end: usize) -> String {
    source
        .get(start..end)
        .unwrap_or_default()
        .trim_start_matches('@')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_controller_yields_its_class_and_methods() {
        let source = "\
import frappe
from frappe.model.document import Document


class LibraryMember(Document):
\tdef validate(self):
\t\tpass

\tdef add_loan(self, title: str, due_date: str) -> None:
\t\tpass
";
        let module = read(source);

        assert_eq!(module.classes.len(), 1);
        assert!(module.functions.is_empty(), "methods are not module-level");

        let class = module.class("LibraryMember").expect("class");
        assert_eq!(class.bases, ["Document"]);
        assert_eq!(
            class
                .methods
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["validate", "add_loan"]
        );
        assert_eq!(
            class.method("add_loan").expect("method").parameters,
            ["self", "title", "due_date"]
        );
        assert_eq!(class.name_range.text(source), Some("LibraryMember"));
    }

    #[test]
    fn whitelist_is_recognized_qualified_bare_and_aliased() {
        let source = "\
import frappe
from frappe import whitelist as w


@frappe.whitelist()
def a():
\tpass


@w()
def b():
\tpass


@frappe.whitelist(allow_guest=True, methods=[\"GET\", \"POST\"])
def c():
\tpass
";
        let module = read(source);
        assert_eq!(module.functions.len(), 3);

        assert!(matches!(
            module.function("a").expect("a").whitelist,
            Whitelist::Yes(_)
        ));
        assert!(matches!(
            module.function("b").expect("b").whitelist,
            Whitelist::Yes(_)
        ));

        let Whitelist::Yes(exposure) = &module.function("c").expect("c").whitelist else {
            panic!("c is whitelisted");
        };
        assert!(exposure.allow_guest);
        assert_eq!(exposure.methods, ["GET", "POST"]);
        assert!(!exposure.xss_safe);
    }

    #[test]
    fn an_undecorated_function_is_certainly_not_whitelisted() {
        let module = read("def plain():\n\tpass\n");
        assert_eq!(
            module.function("plain").expect("plain").whitelist,
            Whitelist::No
        );
    }

    #[test]
    fn a_known_decorator_does_not_make_the_answer_unknown() {
        let source = "\
class X:
\t@staticmethod
\tdef get_list(filters=None):
\t\treturn []
";
        let class = read(source).classes.remove(0);
        assert_eq!(
            class.method("get_list").expect("method").whitelist,
            Whitelist::No
        );
    }

    #[test]
    fn an_unresolvable_decorator_yields_unknown_not_a_denial() {
        // specs/07: indirection must never produce "not whitelisted", because a
        // diagnostic built on that would be a false positive.
        let module = read("@some.registry.expose()\ndef maybe():\n\tpass\n");
        assert_eq!(
            module.function("maybe").expect("maybe").whitelist,
            Whitelist::Unknown
        );
    }

    #[test]
    fn a_def_inside_a_docstring_is_not_a_def() {
        let source = "\
def real():
\t\"\"\"Docstring.

\tdef fake():
\t\tpass

\tclass Fake:
\t\tpass
\t\"\"\"
\treturn 1
";
        let module = read(source);
        assert_eq!(
            module
                .functions
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            ["real"]
        );
        assert!(module.classes.is_empty());
    }

    #[test]
    fn a_commented_out_def_is_not_a_def() {
        let module = read("# def commented():\n#\tpass\ndef real():\n\tpass\n");
        assert_eq!(module.functions.len(), 1);
        assert_eq!(module.functions[0].name, "real");
    }

    #[test]
    fn a_multi_line_signature_is_one_statement() {
        let source = "\
def wide(
\tfirst: str,
\tsecond: dict[str, int] = None,
):
\tpass


def after():
\tpass
";
        let module = read(source);
        assert_eq!(module.functions.len(), 2);
        assert_eq!(
            module.function("wide").expect("wide").parameters,
            ["first", "second"]
        );
        assert!(module.function("after").is_some());
    }

    #[test]
    fn a_definition_range_covers_its_body() {
        let source = "def a():\n\treturn 1\n\n\ndef b():\n\treturn 2\n";
        let module = read(source);
        let a = module.function("a").expect("a");
        let text = a.range.text(source).expect("range text");
        assert!(text.starts_with("def a():"), "{text:?}");
        assert!(text.contains("return 1"));
        assert!(!text.contains("def b"), "{text:?}");
    }

    #[test]
    fn a_decorator_belongs_to_the_definition_range() {
        let source = "import frappe\n\n\n@frappe.whitelist()\ndef ping():\n\treturn 1\n";
        let module = read(source);
        let text = module.function("ping").expect("ping").range.text(source);
        assert_eq!(text, Some("@frappe.whitelist()\ndef ping():\n\treturn 1\n"));
    }

    #[test]
    fn a_nested_def_is_not_a_module_symbol() {
        let source = "def outer():\n\tdef inner():\n\t\tpass\n\treturn inner\n";
        let module = read(source);
        assert_eq!(module.functions.len(), 1);
        assert_eq!(module.functions[0].name, "outer");
    }

    #[test]
    fn an_empty_or_binary_ish_file_yields_nothing() {
        let empty = read("");
        assert!(empty.classes.is_empty());
        assert!(empty.functions.is_empty());

        let binary = read("\u{0}\u{1}\u{2}");
        assert!(binary.classes.is_empty());
        assert!(binary.functions.is_empty());
    }
}
