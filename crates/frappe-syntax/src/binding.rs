//! Local binding inference, bounded exactly as
//! [06](../../../docs/specs/06-analysis-pipeline.md) writes it.
//!
//! Single function scope, literal constructors, drop on **any** reassignment,
//! `self`, and table iteration. No branch merging, no loop analysis, no
//! parameters, no returns. Anything beyond this needs its own spec.

use std::collections::HashMap;

use tree_sitter::Node;

use crate::language::Language;
use crate::matches::{Binding, Span};
use crate::read;
use crate::shapes;

/// Per-file `from frappe import …` names —
/// [10](../../../docs/specs/10-surface-python.md)'s aliasing rule.
///
/// `import frappe as f` is deliberately absent: rare enough that supporting it
/// is not worth the query complexity, and unrecognized is silent, never wrong.
#[derive(Debug, Default)]
pub(crate) struct Imports {
    local_to_dotted: HashMap<String, String>,
}

impl Imports {
    /// Collect `from frappe import get_doc, whitelist as w` across a file.
    pub(crate) fn collect(root: Node<'_>, source: &str) -> Self {
        let mut imports = Self::default();
        let mut cursor = root.walk();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "import_from_statement" {
                imports.collect_one(node, source);
            }
            stack.extend(node.named_children(&mut cursor));
        }
        imports
    }

    fn collect_one(&mut self, node: Node<'_>, source: &str) {
        let Some(module) = node.child_by_field_name("module_name") else {
            return;
        };
        if read::text(module, source) != "frappe" {
            return;
        }
        for child in node.named_children(&mut node.walk()) {
            let (local, imported) = match child.kind() {
                "dotted_name" if child.id() != module.id() => {
                    let name = read::text(child, source);
                    (name, name)
                }
                "aliased_import" => {
                    let (Some(name), Some(alias)) = (
                        child.child_by_field_name("name"),
                        child.child_by_field_name("alias"),
                    ) else {
                        continue;
                    };
                    (read::text(alias, source), read::text(name, source))
                }
                _ => continue,
            };
            self.local_to_dotted
                .insert(local.to_string(), format!("frappe.{imported}"));
        }
    }

    /// The dotted callee a written callee stands for, after import aliasing.
    pub(crate) fn resolve(&self, callee: &str) -> String {
        let (head, tail) = match callee.split_once('.') {
            Some((head, tail)) => (head, Some(tail)),
            None => (callee, None),
        };
        match (self.local_to_dotted.get(head), tail) {
            (Some(dotted), None) => dotted.clone(),
            (Some(dotted), Some(tail)) => format!("{dotted}.{tail}"),
            (None, _) => callee.to_string(),
        }
    }
}

/// One name bound over a byte range.
#[derive(Debug, Clone)]
struct Scoped {
    name: String,
    /// Where the binding holds. Starts after the statement that made it.
    span: Span,
    binding: Binding,
}

/// A function body, or a `frappe.ui.form.on` call in JavaScript.
#[derive(Debug)]
struct Scope {
    span: Span,
    bound: Vec<Scoped>,
}

/// Every binding in a file, indexed by the scope it belongs to.
#[derive(Debug, Default)]
pub(crate) struct Scopes {
    scopes: Vec<Scope>,
}

impl Scopes {
    /// What `name` is bound to at `offset`, searching the innermost scope only
    /// — nested functions do not inherit.
    pub(crate) fn lookup(&self, name: &str, offset: usize) -> Option<&Binding> {
        self.scopes
            .iter()
            .filter(|scope| scope.span.contains(offset))
            .min_by_key(|scope| scope.span.end - scope.span.start)?
            .bound
            .iter()
            .find(|scoped| scoped.name == name && scoped.span.contains(offset))
            .map(|scoped| &scoped.binding)
    }
}

/// What happens to a name at a point in a scope.
enum Event {
    Bind(String, Binding),
    /// Any reassignment, of any shape, drops the binding for the rest of the
    /// scope — [06](../../../docs/specs/06-analysis-pipeline.md).
    Drop(String),
}

/// Build Python function scopes, then finish the table-iteration bindings whose
/// receiver was itself a binding.
pub(crate) fn python_scopes(
    root: Node<'_>,
    source: &str,
    imports: &Imports,
    self_doctype: Option<&str>,
    self_class: Option<&str>,
) -> Scopes {
    let mut scopes = Scopes::default();
    let mut stack = vec![root];
    let mut cursor = root.walk();
    while let Some(node) = stack.pop() {
        stack.extend(node.named_children(&mut cursor));
        if node.kind() != "function_definition" {
            continue;
        }
        let Some(body) = node.child_by_field_name("body") else {
            continue;
        };

        let mut events = Vec::new();
        // `self` in a controller *class* binds to the owning DocType, for the
        // whole body — specs/06. In any other class in the same file it binds
        // to nothing: a helper's `self` is not a document, and treating it as
        // one reports its attributes as missing fields.
        if let Some(doctype) = self_doctype
            && takes_self(node, source)
            && self_class.is_none_or(|class| in_class(node, source, class))
        {
            events.push((
                body.start_byte(),
                Event::Bind("self".to_string(), Binding::DocType(doctype.to_string())),
            ));
        }
        collect_python_events(body, source, imports, &mut events);
        scopes.scopes.push(build(body.into(), events));
    }

    resolve_table_rows(&mut scopes);
    scopes
}

/// Build the `frm` scopes of
/// [11](../../../docs/specs/11-surface-javascript.md): `frm` and `cur_frm` are
/// anchored to the enclosing `frappe.ui.form.on` DocType.
/// Whether `node` sits inside the body of a class called `name`.
fn in_class(node: Node<'_>, source: &str, name: &str) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "class_definition" {
            return parent
                .child_by_field_name("name")
                .is_some_and(|identifier| read::text(identifier, source) == name);
        }
        current = parent.parent();
    }
    false
}

pub(crate) fn javascript_scopes(root: Node<'_>, source: &str) -> Scopes {
    let mut scopes = Scopes::default();
    let mut stack = vec![root];
    let mut cursor = root.walk();
    while let Some(node) = stack.pop() {
        stack.extend(node.named_children(&mut cursor));

        let Some((callee, arguments)) = read::call_parts(node, Language::JavaScript) else {
            continue;
        };
        if read::dotted(callee, source).as_deref() != Some("frappe.ui.form.on") {
            continue;
        }
        let arguments = read::positional(arguments, Language::JavaScript);
        let Some(doctype) = arguments
            .first()
            .and_then(|node| read::literal(*node, source, Language::JavaScript))
        else {
            continue;
        };

        let span: Span = node.into();
        let bound = ["frm", "cur_frm"]
            .into_iter()
            .map(|name| Scoped {
                name: name.to_string(),
                span,
                binding: Binding::DocType(doctype.text.clone()),
            })
            .collect();
        scopes.scopes.push(Scope { span, bound });
    }
    scopes
}

/// Whether the first parameter is `self`, which is what makes a function a
/// method rather than a module-level helper.
fn takes_self(function: Node<'_>, source: &str) -> bool {
    function
        .child_by_field_name("parameters")
        .and_then(|parameters| parameters.named_child(0))
        .is_some_and(|first| read::text(first, source) == "self")
}

/// Walk one function body without descending into nested functions.
fn collect_python_events(
    body: Node<'_>,
    source: &str,
    imports: &Imports,
    events: &mut Vec<(usize, Event)>,
) {
    let mut cursor = body.walk();
    let mut stack: Vec<Node<'_>> = body.named_children(&mut cursor).collect();
    while let Some(node) = stack.pop() {
        // A nested function is its own scope and inherits nothing.
        if matches!(node.kind(), "function_definition" | "lambda") {
            continue;
        }
        stack.extend(node.named_children(&mut cursor));

        match node.kind() {
            "assignment" | "augmented_assignment" => {
                let Some(left) = node.child_by_field_name("left") else {
                    continue;
                };
                let after = node.end_byte();
                let constructed = (node.kind() == "assignment" && left.kind() == "identifier")
                    .then(|| node.child_by_field_name("right"))
                    .flatten()
                    .and_then(|right| constructed_doctype(right, source, imports));

                match constructed {
                    Some(doctype) => events.push((
                        after,
                        Event::Bind(
                            read::text(left, source).to_string(),
                            Binding::DocType(doctype),
                        ),
                    )),
                    None => events.extend(
                        assigned_names(left, source)
                            .into_iter()
                            .map(|name| (after, Event::Drop(name))),
                    ),
                }
            }
            "for_statement" => {
                let (Some(left), Some(right)) = (
                    node.child_by_field_name("left"),
                    node.child_by_field_name("right"),
                ) else {
                    continue;
                };
                // The binding takes effect at the body, not after the loop.
                let after = node
                    .child_by_field_name("body")
                    .map_or_else(|| node.end_byte(), |body| body.start_byte());

                match (left.kind(), table_iteration(right, source)) {
                    ("identifier", Some(binding)) => events.push((
                        after,
                        Event::Bind(read::text(left, source).to_string(), binding),
                    )),
                    _ => events.extend(
                        assigned_names(left, source)
                            .into_iter()
                            .map(|name| (after, Event::Drop(name))),
                    ),
                }
            }
            // `with … as name`, `except … as name` and `name := …` all rebind.
            "as_pattern" | "named_expression" => {
                let target = node
                    .child_by_field_name("alias")
                    .or_else(|| node.child_by_field_name("name"))
                    .or_else(|| node.named_child(1));
                if let Some(target) = target {
                    events.extend(
                        assigned_names(target, source)
                            .into_iter()
                            .map(|name| (node.end_byte(), Event::Drop(name))),
                    );
                }
            }
            _ => {}
        }
    }
}

/// Every plain identifier an assignment target names.
fn assigned_names(target: Node<'_>, source: &str) -> Vec<String> {
    match target.kind() {
        "identifier" => vec![read::text(target, source).to_string()],
        "pattern_list" | "tuple_pattern" | "list_pattern" | "tuple" | "list"
        | "as_pattern_target" => target
            .named_children(&mut target.walk())
            .flat_map(|child| assigned_names(child, source))
            .collect(),
        // `self.x = …` and `items[0] = …` rebind an attribute, not a name.
        _ => Vec::new(),
    }
}

/// The DocType a recognized constructor with a literal argument produces.
fn constructed_doctype(right: Node<'_>, source: &str, imports: &Imports) -> Option<String> {
    let (callee, arguments) = read::call_parts(right, Language::Python)?;
    let written = read::dotted(callee, source)?;
    let shape = shapes::python_shape(&imports.resolve(&written))?;
    if !shape.constructor {
        return None;
    }

    let arguments = read::positional(arguments, Language::Python);
    let first = arguments.first()?;
    // `frappe.get_doc({"doctype": "Loan", …})` binds through the dict form.
    if let Some(pairs) = read::mapping(*first, Language::Python) {
        return pairs.iter().find_map(|(key, value)| {
            let key = read::literal(*key, source, Language::Python)?;
            (key.text == "doctype")
                .then(|| read::literal(*value, source, Language::Python))
                .flatten()
                .map(|literal| literal.text)
        });
    }
    Some(read::literal(*first, source, Language::Python)?.text)
}

/// `for row in self.items:` — the iterated expression must be
/// `<name>.<fieldname>` and nothing else. The receiver is recorded as written
/// and resolved by [`resolve_table_rows`].
fn table_iteration(right: Node<'_>, source: &str) -> Option<Binding> {
    if right.kind() != "attribute" {
        return None;
    }
    let object = right.child_by_field_name("object")?;
    let attribute = right.child_by_field_name("attribute")?;
    if object.kind() != "identifier" {
        return None;
    }
    Some(Binding::TableRow {
        doctype: read::text(object, source).to_string(),
        fieldname: read::text(attribute, source).to_string(),
    })
}

/// Replace the receiver **name** each `TableRow` was recorded with by the
/// DocType it is bound to, and drop the ones whose receiver is unbound.
fn resolve_table_rows(scopes: &mut Scopes) {
    let resolved: Vec<(usize, usize, Option<String>)> = scopes
        .scopes
        .iter()
        .enumerate()
        .flat_map(|(index, scope)| {
            scope
                .bound
                .iter()
                .enumerate()
                .filter_map(|(position, scoped)| match &scoped.binding {
                    Binding::TableRow { doctype, .. } => Some((
                        index,
                        position,
                        match scopes.lookup(doctype, scoped.span.start) {
                            Some(Binding::DocType(name)) => Some(name.clone()),
                            // A row of a row is two joins deep; specs/06 stops
                            // at one.
                            Some(Binding::TableRow { .. }) | None => None,
                        },
                    )),
                    Binding::DocType(_) => None,
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let mut unresolved: Vec<(usize, usize)> = Vec::new();
    for (index, position, receiver) in resolved {
        match receiver {
            Some(name) => {
                if let Binding::TableRow { doctype, .. } =
                    &mut scopes.scopes[index].bound[position].binding
                {
                    *doctype = name;
                }
            }
            None => unresolved.push((index, position)),
        }
    }

    unresolved.sort_unstable();
    for (index, position) in unresolved.into_iter().rev() {
        scopes.scopes[index].bound.remove(position);
    }
}

/// Turn point events into intervals that end at the next event for the same
/// name, or at the end of the scope.
fn build(span: Span, mut events: Vec<(usize, Event)>) -> Scope {
    events.sort_by_key(|(offset, _)| *offset);

    let mut bound: Vec<Scoped> = Vec::new();
    let mut open: HashMap<String, usize> = HashMap::new();

    for (offset, event) in events {
        let name = match &event {
            Event::Bind(name, _) | Event::Drop(name) => name.clone(),
        };
        if let Some(index) = open.remove(&name) {
            bound[index].span.end = offset;
        }
        if let Event::Bind(name, binding) = event {
            open.insert(name.clone(), bound.len());
            bound.push(Scoped {
                name,
                span: Span::new(offset, span.end),
                binding,
            });
        }
    }

    Scope { span, bound }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::{Buffer, PositionEncoding};

    struct Fixture {
        source: String,
        buffer: Buffer,
    }

    impl Fixture {
        fn new(source: &str) -> Self {
            Self {
                source: source.to_string(),
                buffer: Buffer::open(Language::Python, source, 1, PositionEncoding::Utf16)
                    .expect("Python grammar"),
            }
        }

        fn scopes(&self, self_doctype: Option<&str>) -> Scopes {
            let root = self.buffer.tree().root_node();
            let imports = Imports::collect(root, &self.source);
            python_scopes(root, &self.source, &imports, self_doctype, None)
        }

        /// Byte offset just past the first occurrence of `needle`.
        fn after(&self, needle: &str) -> usize {
            self.source.find(needle).expect("needle in fixture") + needle.len()
        }
    }

    #[test]
    fn a_literal_constructor_binds_its_target() {
        let fixture = Fixture::new(
            "def on_submit(self):\n\
             \tinv = frappe.get_doc(\"Sales Invoice\", self.invoice)\n\
             \tinv.customer\n",
        );
        let scopes = fixture.scopes(None);

        assert_eq!(
            scopes.lookup("inv", fixture.after("inv.cust")),
            Some(&Binding::DocType("Sales Invoice".to_string()))
        );
    }

    #[test]
    fn a_non_literal_constructor_binds_nothing() {
        let fixture = Fixture::new(
            "def dynamic(doctype, name):\n\
             \tinv = frappe.get_doc(doctype, name)\n\
             \tinv.customer\n",
        );
        let scopes = fixture.scopes(None);

        assert_eq!(scopes.lookup("inv", fixture.after("inv.cust")), None);
    }

    #[test]
    fn a_module_constant_does_not_travel_into_the_function() {
        let fixture = Fixture::new(
            "D = \"Sales Invoice\"\n\n\
             def constant(name):\n\
             \tinv = frappe.get_doc(D, name)\n\
             \tinv.customer\n",
        );
        let scopes = fixture.scopes(None);

        assert_eq!(scopes.lookup("inv", fixture.after("inv.cust")), None);
    }

    #[test]
    fn any_reassignment_drops_the_binding() {
        let fixture = Fixture::new(
            "def run(self):\n\
             \tinv = frappe.get_doc(\"Sales Invoice\", 1)\n\
             \tinv.customer\n\
             \tinv = something_else()\n\
             \tinv.customer\n",
        );
        let scopes = fixture.scopes(None);

        assert!(scopes.lookup("inv", fixture.after("inv.cust")).is_some());
        assert_eq!(scopes.lookup("inv", fixture.after("()\n\tinv.cust")), None);
    }

    #[test]
    fn a_nested_function_inherits_nothing() {
        let fixture = Fixture::new(
            "def outer(self):\n\
             \tinv = frappe.get_doc(\"Sales Invoice\", 1)\n\n\
             \tdef inner():\n\
             \t\treturn inv.customer\n",
        );
        let scopes = fixture.scopes(None);

        assert_eq!(scopes.lookup("inv", fixture.after("return inv.cust")), None);
    }

    #[test]
    fn self_binds_only_in_a_controller_and_only_in_methods() {
        let source = "class LibraryMember(Document):\n\
                      \tdef validate(self):\n\
                      \t\tself.referred_by\n\n\n\
                      def helper(member):\n\
                      \tself.referred_by\n";
        let fixture = Fixture::new(source);

        let bound = fixture.scopes(Some("Library Member"));
        assert_eq!(
            bound.lookup("self", fixture.after("\t\tself.ref")),
            Some(&Binding::DocType("Library Member".to_string()))
        );
        assert_eq!(bound.lookup("self", fixture.after("\n\tself.ref")), None);

        // Not a controller file: nothing to bind `self` to.
        let unbound = fixture.scopes(None);
        assert_eq!(unbound.lookup("self", fixture.after("\t\tself.ref")), None);
    }

    #[test]
    fn iterating_a_table_field_binds_the_row_to_the_field_not_a_doctype() {
        let fixture = Fixture::new(
            "class LibraryMember(Document):\n\
             \tdef renew_all(self):\n\
             \t\tfor row in self.loans:\n\
             \t\t\trow.loan_status = \"Issued\"\n",
        );
        let scopes = fixture.scopes(Some("Library Member"));

        assert_eq!(
            scopes.lookup("row", fixture.after("\t\t\trow.loan")),
            Some(&Binding::TableRow {
                doctype: "Library Member".to_string(),
                fieldname: "loans".to_string(),
            })
        );
    }

    #[test]
    fn iterating_anything_else_binds_nothing() {
        let fixture = Fixture::new(
            "def run(self, rows):\n\
             \tfor row in rows:\n\
             \t\trow.loan_status = \"Issued\"\n",
        );
        let scopes = fixture.scopes(Some("Library Member"));

        assert_eq!(scopes.lookup("row", fixture.after("\t\trow.loan")), None);
    }

    #[test]
    fn from_frappe_import_is_recognized_but_import_frappe_as_f_is_not() {
        let fixture = Fixture::new(
            "from frappe import get_doc, whitelist as w\n\
             import frappe as f\n\n\
             def run(name):\n\
             \ta = get_doc(\"Library Member\", name)\n\
             \tb = f.get_doc(\"Library Member\", name)\n\
             \treturn a, b\n",
        );
        let scopes = fixture.scopes(None);

        assert_eq!(
            scopes.lookup("a", fixture.after("\treturn a")),
            Some(&Binding::DocType("Library Member".to_string()))
        );
        assert_eq!(scopes.lookup("b", fixture.after("\treturn a")), None);

        let imports = Imports::collect(fixture.buffer.tree().root_node(), &fixture.source);
        assert_eq!(imports.resolve("w"), "frappe.whitelist");
        assert_eq!(imports.resolve("f.get_doc"), "f.get_doc");
    }
}
