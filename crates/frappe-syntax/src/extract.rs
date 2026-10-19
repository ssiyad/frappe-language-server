//! Source text to `(pattern_id, spans)`.
//!
//! No index, no resolution: this walk reads call shapes and string literals and
//! stops where metadata would be needed
//! ([02](../../../docs/specs/02-architecture.md)). Every argument that is not a
//! plain literal yields [`Anchor::Unknown`]
//! ([06](../../../docs/specs/06-analysis-pipeline.md)).

use tree_sitter::{Node, Tree};

use crate::binding::{Imports, Scopes, javascript_scopes, python_scopes};
use crate::language::Language;
use crate::matches::{Anchor, Binding, Match, PathKind, Role, Slot, Span};
use crate::queries::{self, Site};
use crate::read;
use crate::shapes::{self, Arg, Method, Shape, special};

/// What the file's own place in the workspace tells extraction.
///
/// The only outside knowledge this crate takes. It is a name, not an index
/// handle, so extraction stays testable with no bench present
/// ([32](../../../docs/specs/32-testing.md)).
#[derive(Debug, Clone, Copy, Default)]
pub struct Context<'a> {
    /// The DocType a controller file owns. Binds `self`
    /// ([06](../../../docs/specs/06-analysis-pipeline.md)).
    pub self_doctype: Option<&'a str>,
    /// The DocType a `doctype_js` hook bound this file to.
    ///
    /// A bound script need not contain a `frappe.ui.form.on` block at all —
    /// the hook is what says which form it is for
    /// ([12](../../../docs/specs/12-surface-hooks.md)).
    pub frm_doctype: Option<&'a str>,
    /// The controller class `self` belongs to. A controller file holds other
    /// classes too — erpnext's `stock_entry_type.py` carries a plain
    /// `ManufactureEntry` beside the `Document` subclass — and their `self` is
    /// not a document at all.
    pub self_class: Option<&'a str>,
}

/// Every recognized shape in a parsed buffer, in source order.
#[must_use]
pub fn extract(language: Language, tree: &Tree, source: &str, context: Context<'_>) -> Vec<Match> {
    // A template's tree is a Python parse of its expressions, and the spans in
    // it are spans in the template, so the template text reads as Python too.
    let language = language.reads_as();
    let root = tree.root_node();
    let mut extractor = match language {
        Language::Python | Language::Jinja => {
            let imports = Imports::collect(root, source);
            let scopes = python_scopes(
                root,
                source,
                &imports,
                context.self_doctype,
                context.self_class,
            );
            Extractor {
                language,
                source,
                imports,
                scopes,
                matches: Vec::new(),
            }
        }
        Language::JavaScript => Extractor {
            language,
            source,
            imports: Imports::default(),
            scopes: javascript_scopes(root, source, context.frm_doctype),
            matches: Vec::new(),
        },
        // DocType JSON carries no call shapes; the index reads it with
        // `serde_json` and the tree exists only for in-buffer positions.
        Language::Json => return Vec::new(),
    };

    extractor.walk(language, tree, source);
    extractor.matches.sort_by_key(|found| found.span);
    extractor.matches
}

struct Extractor<'a> {
    language: Language,
    source: &'a str,
    imports: Imports,
    scopes: Scopes,
    matches: Vec<Match>,
}

impl<'a> Extractor<'a> {
    /// Layer one locates the sites, layer two reads their shapes — spec 06.
    /// Nothing here asks a node what kind it is; the query already answered.
    fn walk(&mut self, language: Language, tree: &'a Tree, source: &str) {
        for (site, node) in queries::sites(language, tree, source) {
            match (language, site) {
                (Language::Python, Site::Decorator) => self.python_decorator(node),
                (Language::Python, Site::Call) => self.python_call(node),
                (Language::Python, Site::Attribute) => self.python_attribute(node),
                (Language::JavaScript, Site::Call) => self.javascript_call(node),
                (Language::JavaScript, Site::Attribute) => self.javascript_member(node),
                (Language::JavaScript, Site::Assignment) => self.javascript_assignment(node),
                (Language::JavaScript, Site::Subscript) => self.javascript_subscript(node),
                _ => {}
            }
        }
    }

    // --- Python --------------------------------------------------------------

    /// `@frappe.whitelist(...)` — [07](../../../docs/specs/07-dotted-paths.md).
    fn python_decorator(&mut self, node: Node<'a>) {
        let Some(inner) = node.named_child(0) else {
            return;
        };
        let callee = match read::call_parts(inner, Language::Python) {
            Some((callee, _)) => callee,
            None => inner,
        };
        let Some(written) = read::dotted(callee, self.source) else {
            return;
        };
        if self.imports.resolve(&written) != "frappe.whitelist" {
            return;
        }
        self.matches.push(Match {
            pattern: special::PY_WHITELIST,
            span: node.into(),
            anchor: Anchor::Unknown,
            receiver: None,
            slots: Vec::new(),
        });
    }

    fn python_call(&mut self, node: Node<'a>) {
        let Some((callee, arguments)) = read::call_parts(node, Language::Python) else {
            return;
        };
        let Some(written) = read::dotted(callee, self.source) else {
            return;
        };
        let resolved = self.imports.resolve(&written);

        // `_("…")` and `frappe._("…")` — [23](../../../docs/specs/23-diagnostics.md).
        if written == "_" || resolved == "frappe._" {
            self.translation(node, arguments, special::PY_TRANSLATE);
            return;
        }

        if let Some(shape) = shapes::python_shape(&resolved) {
            self.shape_call(node, arguments, shape);
            return;
        }

        // A method on `self` or on a name bound per specs/06.
        let (Some(object), Some(name)) = (
            callee.child_by_field_name("object"),
            callee.child_by_field_name("attribute"),
        ) else {
            return;
        };
        if !read::is_name(object) {
            return;
        }
        let Some(binding) = self.binding(read::text(object, self.source), node.start_byte()) else {
            return;
        };
        let name = read::text(name, self.source);
        if let Some(method) = shapes::PYTHON_METHODS.iter().find(|m| m.name == name) {
            self.method_call(node, arguments, method, binding);
        }
    }

    /// `doc.<attr>` on a bound name —
    /// [10](../../../docs/specs/10-surface-python.md).
    fn python_attribute(&mut self, node: Node<'a>) {
        // The method table owns `doc.append(…)`; this is bare attribute access.
        if node
            .parent()
            .and_then(|parent| parent.child_by_field_name("function"))
            .is_some_and(|function| function.id() == node.id())
        {
            return;
        }
        let (Some(object), Some(name)) = (
            node.child_by_field_name("object"),
            node.child_by_field_name("attribute"),
        ) else {
            return;
        };
        if !read::is_name(object) {
            return;
        }
        let Some(binding) = self.binding(read::text(object, self.source), node.start_byte()) else {
            return;
        };

        let anchor = binding.anchor();
        let fieldname = read::text(name, self.source).to_string();
        self.matches.push(Match {
            pattern: special::PY_ATTRIBUTE,
            span: node.into(),
            anchor: anchor.clone(),
            receiver: Some(binding),
            slots: vec![Slot {
                role: Role::Field,
                span: name.into(),
                text: Some(fieldname.clone()),
                anchor: field_anchor(&anchor, &fieldname),
            }],
        });
    }

    // --- JavaScript ----------------------------------------------------------

    fn javascript_call(&mut self, node: Node<'a>) {
        let Some((callee, arguments)) = read::call_parts(node, Language::JavaScript) else {
            return;
        };
        let Some(written) = read::dotted(callee, self.source) else {
            return;
        };

        match written.as_str() {
            "__" => return self.translation(node, arguments, special::JS_TRANSLATE),
            "frappe.ui.form.on" => return self.form_on(node, arguments),
            "frappe.call" => return self.method_object(node, arguments, special::JS_CALL, None),
            "frappe.set_route" => return self.set_route(node, arguments),
            _ => {}
        }

        if let Some(shape) = shapes::javascript_shape(&written) {
            self.shape_call(node, arguments, shape);
            return;
        }

        let (Some(object), Some(name)) = (
            callee.child_by_field_name("object"),
            callee.child_by_field_name("property"),
        ) else {
            return;
        };
        if !read::is_name(object) {
            return;
        }
        let Some(binding) = self.binding(read::text(object, self.source), node.start_byte()) else {
            return;
        };

        match read::text(name, self.source) {
            "set_query" => self.set_query(node, arguments, binding),
            "call" => self.method_object(node, arguments, special::JS_FRM_CALL, Some(binding)),
            name => {
                if let Some(method) = shapes::JAVASCRIPT_METHODS.iter().find(|m| m.name == name) {
                    self.method_call(node, arguments, method, binding);
                }
            }
        }
    }

    /// `frm.doc.<attr>` — [11](../../../docs/specs/11-surface-javascript.md).
    fn javascript_member(&mut self, node: Node<'a>) {
        let (Some(object), Some(name)) = (
            node.child_by_field_name("object"),
            node.child_by_field_name("property"),
        ) else {
            return;
        };
        if !read::is_member(object) {
            return;
        }
        let (Some(form), Some(doc)) = (
            object.child_by_field_name("object"),
            object.child_by_field_name("property"),
        ) else {
            return;
        };
        if !read::is_name(form) || read::text(doc, self.source) != "doc" {
            return;
        }
        let Some(binding) = self.binding(read::text(form, self.source), node.start_byte()) else {
            return;
        };

        let anchor = binding.anchor();
        let fieldname = read::text(name, self.source).to_string();
        self.matches.push(Match {
            pattern: special::JS_FRM_DOC,
            span: node.into(),
            anchor: anchor.clone(),
            receiver: Some(binding),
            slots: vec![Slot {
                role: Role::Field,
                span: name.into(),
                text: Some(fieldname.clone()),
                anchor: field_anchor(&anchor, &fieldname),
            }],
        });
    }

    /// `frappe.listview_settings[D] = {…}`.
    fn javascript_assignment(&mut self, node: Node<'a>) {
        let Some(left) = node.child_by_field_name("left") else {
            return;
        };
        if !read::is_subscript(left) {
            return;
        }
        let (Some(object), Some(index)) = (
            left.child_by_field_name("object"),
            left.child_by_field_name("index"),
        ) else {
            return;
        };
        if read::dotted(object, self.source).as_deref() != Some("frappe.listview_settings") {
            return;
        }

        let mut slots = Vec::new();
        let anchor = self.doctype_slot(index, &mut slots);
        self.matches.push(Match {
            pattern: special::JS_LISTVIEW_SETTINGS,
            span: node.into(),
            anchor,
            receiver: None,
            slots,
        });
    }

    /// `locals[D][name]`.
    fn javascript_subscript(&mut self, node: Node<'a>) {
        let Some(inner) = node.child_by_field_name("object") else {
            return;
        };
        if !read::is_subscript(inner) {
            return;
        }
        let (Some(object), Some(index)) = (
            inner.child_by_field_name("object"),
            inner.child_by_field_name("index"),
        ) else {
            return;
        };
        if read::text(object, self.source) != "locals" {
            return;
        }

        let mut slots = Vec::new();
        let mut anchor = self.doctype_slot(index, &mut slots);
        // `locals[":Company"]` is frappe's own notation for a Single's cached
        // record; the DocType is what follows the colon. Without this the
        // client scripts that read defaults report a DocType nobody has.
        if let Some(slot) = slots.last_mut()
            && let Some(text) = slot.text.as_ref().and_then(|t| t.strip_prefix(':'))
        {
            let text = text.to_string();
            slot.span = Span {
                start: slot.span.start + 1,
                end: slot.span.end,
            };
            slot.anchor = Anchor::DocType(text.clone());
            slot.text = Some(text.clone());
            anchor = Anchor::DocType(text);
        }
        self.matches.push(Match {
            pattern: special::JS_LOCALS,
            span: node.into(),
            anchor,
            receiver: None,
            slots,
        });
    }

    /// `frappe.ui.form.on(D, {…})` and the 3-argument legacy form.
    fn form_on(&mut self, node: Node<'a>, arguments: Node<'a>) {
        let arguments = read::positional(arguments, Language::JavaScript);
        let mut slots = Vec::new();
        let Some(first) = arguments.first() else {
            return;
        };
        let anchor = self.doctype_slot(*first, &mut slots);

        match arguments.len() {
            // `frappe.ui.form.on(D, event, handler)`. The middle argument is
            // the same namespace as a dict key — a lifecycle event, a
            // fieldname or a table event — so it is read as one. Reading it as
            // a fieldname reported `("Payment Request", "refresh", …)` as a
            // missing field ([32](../../../docs/specs/32-testing.md)).
            3 => {
                let text = self.key_name(arguments[1]);
                slots.push(Slot {
                    role: Role::FormEvent,
                    span: self
                        .literal(arguments[1])
                        .map_or_else(|| arguments[1].into(), |literal| literal.span),
                    text,
                    anchor: Anchor::Unknown,
                });
            }
            _ => {
                if let Some(handlers) = arguments.get(1) {
                    self.form_events(*handlers, &mut slots);
                }
            }
        }

        self.matches.push(Match {
            pattern: special::JS_FORM_ON,
            span: node.into(),
            anchor,
            receiver: None,
            slots,
        });
    }

    /// The keys of a handler object: lifecycle names, fieldnames and table
    /// events share one keyspace
    /// ([11](../../../docs/specs/11-surface-javascript.md)).
    fn form_events(&mut self, handlers: Node<'a>, slots: &mut Vec<Slot>) {
        if !read::is_object(handlers) {
            return;
        }
        for child in handlers.named_children(&mut handlers.walk()) {
            let Some(key) = read::entry_key(child) else {
                continue;
            };

            let text = self.key_name(key);
            slots.push(Slot {
                role: Role::FormEvent,
                span: key.into(),
                text,
                // An event name is neither a DocType nor a field until the
                // schema says which; the match's own anchor carries the DocType.
                anchor: Anchor::Unknown,
            });
        }
    }

    /// `frappe.set_route("Form", D, name)` — only the `"Form"` route names a
    /// DocType in argument 1.
    fn set_route(&mut self, node: Node<'a>, arguments: Node<'a>) {
        let arguments = read::positional(arguments, Language::JavaScript);
        let is_form = arguments
            .first()
            .and_then(|first| read::literal(*first, self.source, Language::JavaScript))
            .is_some_and(|literal| literal.text == "Form");
        let (true, Some(doctype)) = (is_form, arguments.get(1)) else {
            return;
        };

        let mut slots = Vec::new();
        let anchor = self.doctype_slot(*doctype, &mut slots);
        self.matches.push(Match {
            pattern: special::JS_SET_ROUTE,
            span: node.into(),
            anchor,
            receiver: None,
            slots,
        });
    }

    /// `frappe.call({ method, args })` and `frm.call({ method, args })`.
    fn method_object(
        &mut self,
        node: Node<'a>,
        arguments: Node<'a>,
        pattern: crate::matches::PatternId,
        receiver: Option<Binding>,
    ) {
        let arguments = read::positional(arguments, Language::JavaScript);
        let anchor = receiver.as_ref().map_or(Anchor::Unknown, Binding::anchor);
        let mut slots = Vec::new();

        if let Some(pairs) = arguments
            .first()
            .and_then(|first| read::mapping(*first, Language::JavaScript))
        {
            for (key, value) in pairs {
                if self.key_name(key).as_deref() != Some("method") {
                    continue;
                }
                // `frm.call` names a controller method; `frappe.call` a
                // whitelisted dotted path — specs/11.
                let arg = if receiver.is_some() {
                    Arg::Method
                } else {
                    Arg::Path(PathKind::Whitelisted)
                };
                self.push_arg(value, arg, &anchor, &mut slots);
            }
        }

        if slots.is_empty() {
            return;
        }
        self.matches.push(Match {
            pattern,
            span: node.into(),
            anchor,
            receiver,
            slots,
        });
    }

    /// `frm.set_query(F, fn)` binds `F` on the form's DocType; the 3-argument
    /// form binds `F` on the child behind `table_F`
    /// ([11](../../../docs/specs/11-surface-javascript.md)).
    fn set_query(&mut self, node: Node<'a>, arguments: Node<'a>, binding: Binding) {
        let arguments = read::positional(arguments, Language::JavaScript);
        let anchor = binding.anchor();
        let mut slots = Vec::new();

        match arguments.len() {
            3 => {
                self.push_field(arguments[0], &Anchor::Unknown, Role::ChildField, &mut slots);
                self.push_field(arguments[1], &anchor, Role::Field, &mut slots);
            }
            _ => {
                if let Some(first) = arguments.first() {
                    self.push_field(*first, &anchor, Role::Field, &mut slots);
                }
            }
        }

        self.matches.push(Match {
            pattern: special::JS_SET_QUERY,
            span: node.into(),
            anchor,
            receiver: Some(binding),
            slots,
        });
    }

    // --- Shared --------------------------------------------------------------

    fn translation(
        &mut self,
        node: Node<'a>,
        arguments: Node<'a>,
        pattern: crate::matches::PatternId,
    ) {
        let arguments = read::positional(arguments, self.language);
        let Some(first) = arguments.first() else {
            return;
        };
        let kind = read::translation_arg(*first, self.source, self.language);
        let literal = read::literal(*first, self.source, self.language);

        self.matches.push(Match {
            pattern,
            span: node.into(),
            anchor: Anchor::Unknown,
            receiver: None,
            slots: vec![Slot {
                role: Role::Translation(kind),
                span: (*first).into(),
                text: literal.map(|literal| literal.text),
                anchor: Anchor::Unknown,
            }],
        });
    }

    fn shape_call(&mut self, node: Node<'a>, arguments: Node<'a>, shape: &Shape) {
        let positional = read::positional(arguments, self.language);

        // `frappe.get_doc({"doctype": D, F: …})` is its own shape.
        if shape.callee() == "frappe.get_doc"
            && let Some(first) = positional.first()
            && let Some(pairs) = read::mapping(*first, self.language)
        {
            return self.get_doc_dict(node, &pairs);
        }

        let anchor = self.call_anchor(shape.args, &positional);
        let mut slots = Vec::new();
        for (index, arg) in shape.args.iter().enumerate() {
            if let Some(node) = positional.get(index) {
                self.push_arg(*node, *arg, &anchor, &mut slots);
            }
        }
        if shape.kwargs && self.language == Language::Python {
            for (name, value) in read::keyword_arguments(arguments, self.source) {
                if let Some(arg) = shapes::kwarg(name) {
                    self.push_arg(value, arg, &anchor, &mut slots);
                }
            }
        }

        slots.sort_by_key(|slot| slot.span);
        self.matches.push(Match {
            pattern: shape.id,
            span: node.into(),
            anchor,
            receiver: None,
            slots,
        });
    }

    fn get_doc_dict(&mut self, node: Node<'a>, pairs: &[(Node<'a>, Node<'a>)]) {
        let named_doctype = pairs
            .iter()
            .find(|(key, _)| self.key_name(*key).as_deref() == Some("doctype"));
        let anchor = named_doctype
            .and_then(|(_, value)| self.literal_text(*value))
            .map_or(Anchor::Unknown, Anchor::DocType);

        let mut slots = Vec::new();
        for (key, value) in pairs {
            // The `doctype` key names the DocType; every other key is one of
            // its fields — specs/10.
            if self.key_name(*key).as_deref() == Some("doctype") {
                self.push_arg(*value, Arg::DocType, &anchor, &mut slots);
            } else {
                self.push_field(*key, &anchor, Role::Field, &mut slots);
            }
        }

        slots.sort_by_key(|slot| slot.span);
        self.matches.push(Match {
            pattern: special::PY_GET_DOC_DICT,
            span: node.into(),
            anchor,
            receiver: None,
            slots,
        });
    }

    fn method_call(
        &mut self,
        node: Node<'a>,
        arguments: Node<'a>,
        method: &Method,
        binding: Binding,
    ) {
        let positional = read::positional(arguments, self.language);
        let anchor = binding.anchor();
        let mut slots = Vec::new();
        for (index, arg) in method.args.iter().enumerate() {
            if let Some(node) = positional.get(index) {
                self.push_arg(*node, *arg, &anchor, &mut slots);
            }
        }

        slots.sort_by_key(|slot| slot.span);
        self.matches.push(Match {
            pattern: method.id,
            span: node.into(),
            anchor,
            receiver: Some(binding),
            slots,
        });
    }

    /// The DocType the whole call is about: the first `D` argument that is a
    /// literal, and `Unknown` otherwise.
    fn call_anchor(&self, args: &[Arg], positional: &[Node<'a>]) -> Anchor {
        args.iter()
            .enumerate()
            .filter(|(_, arg)| **arg == Arg::DocType)
            .find_map(|(index, _)| self.literal_text(*positional.get(index)?))
            .map_or(Anchor::Unknown, Anchor::DocType)
    }

    fn push_arg(&mut self, node: Node<'a>, arg: Arg, anchor: &Anchor, slots: &mut Vec<Slot>) {
        match arg {
            Arg::Ignore => {}
            Arg::DocType => {
                self.doctype_slot(node, slots);
            }
            Arg::Field => self.push_field(node, anchor, Role::Field, slots),
            Arg::OrderBy => self.push_order_by(node, anchor, slots),
            Arg::Fields => self.push_fields(node, anchor, slots),
            Arg::FieldsOrMap => match read::mapping(node, self.language) {
                Some(pairs) => {
                    for (key, _) in pairs {
                        self.push_field(key, anchor, Role::Field, slots);
                    }
                }
                None => self.push_fields(node, anchor, slots),
            },
            Arg::Filters => self.push_filters(node, anchor, slots),
            Arg::Options => {
                if let Some(pairs) = read::mapping(node, self.language) {
                    for (key, value) in pairs {
                        if let Some(arg) = self.key_name(key).as_deref().and_then(shapes::kwarg) {
                            self.push_arg(value, arg, anchor, slots);
                        }
                    }
                }
            }
            Arg::ChildFields => {
                if let Some(pairs) = read::mapping(node, self.language) {
                    for (key, _) in pairs {
                        // The child DocType is a schema lookup, so these carry
                        // no anchor — specs/06.
                        self.push_field(key, &Anchor::Unknown, Role::ChildField, slots);
                    }
                }
            }
            Arg::Path(kind) => slots.push(match self.literal(node) {
                Some(literal) => Slot {
                    role: Role::Path(kind),
                    span: literal.span,
                    text: Some(literal.text.clone()),
                    anchor: Anchor::DottedPath(literal.text),
                },
                None => Slot::unknown(Role::Path(kind), node.into()),
            }),
            Arg::Method | Arg::HookKey | Arg::Property => {
                let role = match arg {
                    Arg::Method => Role::Method,
                    Arg::HookKey => Role::HookKey,
                    _ => Role::Property,
                };
                // A method name, a hook key and a field property are all
                // checked against tables the index or specs/31 hold, never
                // against a DocType — so none of them anchors here.
                slots.push(match self.literal(node) {
                    Some(literal) => Slot {
                        role,
                        span: literal.span,
                        text: Some(literal.text),
                        anchor: Anchor::Unknown,
                    },
                    None => Slot::unknown(role, node.into()),
                });
            }
        }
    }

    fn doctype_slot(&mut self, node: Node<'a>, slots: &mut Vec<Slot>) -> Anchor {
        let literal = self.literal(node);
        let span = literal.as_ref().map_or_else(|| node.into(), |l| l.span);
        let anchor = literal
            .as_ref()
            .map_or(Anchor::Unknown, |l| Anchor::DocType(l.text.clone()));
        slots.push(Slot {
            role: Role::DocType,
            span,
            text: literal.map(|literal| literal.text),
            anchor: anchor.clone(),
        });
        anchor
    }

    /// A fieldname position. Entries the surface deliberately skips —
    /// aggregates, dotted, backticked, function-wrapped — produce **no slot**
    /// rather than an unknown one ([10](../../../docs/specs/10-surface-python.md)).
    fn push_field(&mut self, node: Node<'a>, anchor: &Anchor, role: Role, slots: &mut Vec<Slot>) {
        let Some(name) = self.key_name(node) else {
            slots.push(Slot::unknown(role, node.into()));
            return;
        };
        if !is_plain_fieldname(&name) {
            return;
        }
        slots.push(Slot {
            role,
            span: self.literal(node).map_or_else(|| node.into(), |l| l.span),
            anchor: if role == Role::Field {
                field_anchor(anchor, &name)
            } else {
                Anchor::Unknown
            },
            text: Some(name),
        });
    }

    fn push_fields(&mut self, node: Node<'a>, anchor: &Anchor, slots: &mut Vec<Slot>) {
        match read::sequence(node, self.language) {
            Some(items) => {
                for item in items {
                    self.push_field(item, anchor, Role::Field, slots);
                }
            }
            None => self.push_field(node, anchor, Role::Field, slots),
        }
    }

    /// `"due_date asc, name desc"` — one literal holding several fieldnames.
    fn push_order_by(&mut self, node: Node<'a>, anchor: &Anchor, slots: &mut Vec<Slot>) {
        let Some(literal) = read::literal(node, self.source, self.language) else {
            slots.push(Slot::unknown(Role::Field, node.into()));
            return;
        };
        // Adjacent-concatenated literals have content that is not contiguous,
        // so no sub-span inside them is trustworthy; skip rather than report a
        // range that points at the wrong bytes — specs/10.
        if literal.content.end - literal.content.start != literal.text.len() {
            return;
        }
        let base = literal.content.start;
        let mut offset = 0;
        for part in literal.text.split(',') {
            let leading = part.len() - part.trim_start().len();
            let name = part.trim();
            let name = name.split_whitespace().next().unwrap_or("");
            if is_plain_fieldname(name) && !name.is_empty() {
                let start = base + offset + leading;
                slots.push(Slot {
                    role: Role::Field,
                    span: Span::new(start, start + name.len()),
                    text: Some(name.to_string()),
                    anchor: field_anchor(anchor, name),
                });
            }
            offset += part.len() + 1;
        }
    }

    /// `{F: v}`, `[[F, op, v], …]` and `[[D, F, op, v], …]`; the 4-tuple form
    /// re-anchors to its own DocType
    /// ([10](../../../docs/specs/10-surface-python.md)).
    fn push_filters(&mut self, node: Node<'a>, anchor: &Anchor, slots: &mut Vec<Slot>) {
        if let Some(pairs) = read::mapping(node, self.language) {
            for (key, _) in pairs {
                self.push_field(key, anchor, Role::Field, slots);
            }
            return;
        }
        let Some(items) = read::sequence(node, self.language) else {
            return;
        };
        let nested: Vec<Vec<Node<'a>>> = items
            .iter()
            .filter_map(|item| read::sequence(*item, self.language))
            .collect();

        if nested.is_empty() {
            self.push_filter_tuple(&items, anchor, slots);
            return;
        }
        for tuple in nested {
            self.push_filter_tuple(&tuple, anchor, slots);
        }
    }

    fn push_filter_tuple(&mut self, tuple: &[Node<'a>], anchor: &Anchor, slots: &mut Vec<Slot>) {
        match tuple.len() {
            4 => {
                let own = self.doctype_slot(tuple[0], slots);
                self.push_field(tuple[1], &own, Role::Field, slots);
            }
            3 => self.push_field(tuple[0], anchor, Role::Field, slots),
            _ => {}
        }
    }

    fn binding(&self, name: &str, offset: usize) -> Option<Binding> {
        self.scopes.lookup(name, offset).cloned()
    }

    /// A name written either as a string literal or as a bare JavaScript
    /// property key — `{"status": …}` and `{ status: … }` mean the same thing.
    ///
    /// Python's bare dict keys are `identifier` nodes and are deliberately not
    /// accepted: `{fieldname: value}` is a variable, not a name.
    fn key_name(&self, key: Node<'a>) -> Option<String> {
        self.literal_text(key)
            .or_else(|| read::bare_key(key).then(|| read::text(key, self.source).to_string()))
    }

    fn literal(&self, node: Node<'a>) -> Option<read::Literal> {
        read::literal(node, self.source, self.language)
    }

    fn literal_text(&self, node: Node<'a>) -> Option<String> {
        self.literal(node).map(|literal| literal.text)
    }
}

/// A field position anchors only when its DocType did —
/// [06](../../../docs/specs/06-analysis-pipeline.md).
fn field_anchor(anchor: &Anchor, fieldname: &str) -> Anchor {
    match anchor {
        Anchor::DocType(doctype) => Anchor::Field(doctype.clone(), fieldname.to_string()),
        _ => Anchor::Unknown,
    }
}

/// Frappe fieldnames are `[A-Za-z0-9_]`. Everything else in a fieldname
/// position — `count(name) as n`, `customer.name`, `` `tabItem`.name `` — is a
/// form [10](../../../docs/specs/10-surface-python.md) skips rather than
/// reports.
///
/// The empty string passes: it is what an editor sees mid-completion.
fn is_plain_fieldname(text: &str) -> bool {
    text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::{Buffer, PositionEncoding};
    use crate::matches::{Match, PatternId, TranslationArg};

    fn matched(language: Language, source: &str, self_doctype: Option<&str>) -> Vec<Match> {
        let buffer =
            Buffer::open(language, source, 1, PositionEncoding::Utf16).expect("a pinned grammar");
        extract(
            language,
            buffer.tree(),
            source,
            Context {
                frm_doctype: None,
                self_doctype,
                self_class: None,
            },
        )
    }

    fn python(source: &str) -> Vec<Match> {
        matched(Language::Python, source, None)
    }

    fn controller(source: &str, self_doctype: &str) -> Vec<Match> {
        matched(Language::Python, source, Some(self_doctype))
    }

    fn javascript(source: &str) -> Vec<Match> {
        matched(Language::JavaScript, source, None)
    }

    /// Every match of one pattern, in source order.
    fn of<'a>(found: &'a [Match], pattern: &'static str) -> Vec<&'a Match> {
        found
            .iter()
            .filter(|found| found.pattern == PatternId(pattern))
            .collect()
    }

    fn fields(found: &Match) -> Vec<(&str, &Anchor)> {
        found
            .slots
            .iter()
            .filter(|slot| slot.role == Role::Field)
            .map(|slot| (slot.text.as_deref().unwrap_or(""), &slot.anchor))
            .collect()
    }

    fn doctype(name: &str) -> Anchor {
        Anchor::DocType(name.to_string())
    }

    fn field(doctype: &str, fieldname: &str) -> Anchor {
        Anchor::Field(doctype.to_string(), fieldname.to_string())
    }

    fn form(doctype: &str) -> Option<Binding> {
        Some(Binding::Form(doctype.to_string()))
    }

    #[test]
    fn a_literal_doctype_anchors_the_call() {
        let found = python("frappe.get_all(\"Sales Invoice\", fields=[\"customer\"])\n");
        let call = of(&found, "frappe.get_all")[0];

        assert_eq!(call.anchor, doctype("Sales Invoice"));
        assert_eq!(
            fields(call),
            [("customer", &field("Sales Invoice", "customer"))]
        );
    }

    #[test]
    fn the_dynamic_argument_forms_of_specs_06_anchor_to_nothing() {
        for source in [
            "frappe.get_all(dt, fields=cols)\n",
            "frappe.get_all(DOCTYPE_CONST, fields=[\"customer\"])\n",
            "frappe.get_all(f\"Sales {kind}\", fields=[\"customer\"])\n",
        ] {
            let found = python(source);
            let call = of(&found, "frappe.get_all")[0];

            assert_eq!(call.anchor, Anchor::Unknown, "{source}");
            for (_, anchor) in fields(call) {
                assert_eq!(*anchor, Anchor::Unknown, "{source}");
            }
        }
    }

    #[test]
    fn a_field_position_on_an_unknown_doctype_contributes_nothing() {
        let found = python("frappe.get_all(dt, fields=[\"customer\"], pluck=\"name\")\n");
        let call = of(&found, "frappe.get_all")[0];

        assert!(!call.slots.is_empty());
        assert!(call.slots.iter().all(|slot| !slot.anchor.is_known()));
    }

    #[test]
    fn the_kwargs_of_specs_10_are_fieldnames_and_the_rest_are_not() {
        let found = python(
            "frappe.get_all(\n\
             \t\"Loan\",\n\
             \tfields=[\"status\"],\n\
             \tfilters={\"member\": m},\n\
             \tor_filters=[[\"status\", \"=\", \"Open\"]],\n\
             \tpluck=\"name\",\n\
             \tgroup_by=\"member\",\n\
             \tlimit_page_length=20,\n\
             \tas_dict=True,\n)\n",
        );
        let call = of(&found, "frappe.get_all")[0];

        let names: Vec<&str> = fields(call).into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["status", "member", "status", "name", "member"]);
    }

    #[test]
    fn entries_specs_10_skips_produce_no_slot_at_all() {
        let found = python(
            "frappe.get_all(\"Loan\", fields=[\"count(name) as n\", \"member.name\", \"`tabLoan`.status\", \"status\"])\n",
        );
        let call = of(&found, "frappe.get_all")[0];

        assert_eq!(fields(call), [("status", &field("Loan", "status"))]);
    }

    #[test]
    fn the_four_tuple_filter_form_reanchors_to_its_own_doctype() {
        let found = python(
            "frappe.get_all(\"Loan\", filters=[[\"Library Loan Item\", \"loan_status\", \"=\", \"Issued\"]])\n",
        );
        let call = of(&found, "frappe.get_all")[0];

        assert_eq!(
            fields(call),
            [("loan_status", &field("Library Loan Item", "loan_status"))]
        );
    }

    #[test]
    fn the_three_tuple_filter_form_uses_the_calls_own_doctype() {
        let found =
            python("frappe.get_all(\"Loan\", filters=[[\"loan_status\", \"=\", \"Issued\"]])\n");
        let call = of(&found, "frappe.get_all")[0];

        assert_eq!(
            fields(call),
            [("loan_status", &field("Loan", "loan_status"))]
        );
    }

    #[test]
    fn order_by_splits_into_one_position_per_fieldname() {
        let source = "frappe.get_all(\"Loan\", order_by=\"due_date asc, member desc\")\n";
        let found = python(source);
        let call = of(&found, "frappe.get_all")[0];

        assert_eq!(
            fields(call),
            [
                ("due_date", &field("Loan", "due_date")),
                ("member", &field("Loan", "member")),
            ]
        );
        // The spans point at the fieldnames inside the one literal.
        let spans: Vec<&str> = call
            .slots
            .iter()
            .filter(|slot| slot.role == Role::Field)
            .map(|slot| &source[slot.span.range()])
            .collect();
        assert_eq!(spans, ["due_date", "member"]);
    }

    #[test]
    fn raw_sql_is_excluded_and_matches_nothing() {
        let found = python("frappe.db.sql(\"select emial_id from `tabLibrary Member`\")\n");
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_constructor_binds_a_local_and_document_methods_follow_it() {
        let found = python(
            "def on_submit(self):\n\
             \tinv = frappe.get_doc(\"Sales Invoice\", 1)\n\
             \tinv.customer\n\
             \tinv.db_set(\"status\", \"Paid\")\n",
        );

        assert_eq!(
            of(&found, "doc.attribute")[0].slots[0].anchor,
            field("Sales Invoice", "customer")
        );
        assert_eq!(
            of(&found, "doc.db_set")[0].slots[0].anchor,
            field("Sales Invoice", "status")
        );
    }

    #[test]
    fn an_unbound_name_produces_no_document_method_match() {
        let found = python(
            "def on_submit(self, inv):\n\
             \tinv.customer\n\
             \tinv.db_set(\"status\", \"Paid\")\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn self_in_a_controller_anchors_and_outside_one_does_not() {
        let source = "class Loan(Document):\n\
                      \tdef validate(self):\n\
                      \t\tself.db_set(\"status\", \"Open\")\n";

        let anchored = controller(source, "Loan");
        assert_eq!(
            of(&anchored, "doc.db_set")[0].slots[0].anchor,
            field("Loan", "status")
        );

        assert!(matched(Language::Python, source, None).is_empty());
    }

    #[test]
    fn append_dict_keys_are_child_fields_that_anchor_to_nothing() {
        let found = controller(
            "class Loan(Document):\n\
             \tdef add(self):\n\
             \t\tself.append(\"items\", {\"book_title\": t})\n",
            "Loan",
        );
        let call = of(&found, "doc.append")[0];

        assert_eq!(call.slots[0].anchor, field("Loan", "items"));
        assert_eq!(call.slots[1].role, Role::ChildField);
        // The child DocType behind `items` is a schema lookup — specs/06.
        assert_eq!(call.slots[1].anchor, Anchor::Unknown);
        assert_eq!(call.slots[1].text.as_deref(), Some("book_title"));
    }

    #[test]
    fn a_table_row_binding_names_the_field_and_anchors_to_nothing() {
        let found = controller(
            "class Loan(Document):\n\
             \tdef renew(self):\n\
             \t\tfor row in self.items:\n\
             \t\t\trow.loan_status\n",
            "Loan",
        );
        let access = of(&found, "doc.attribute");
        let row = access
            .iter()
            .find(|found| found.slots[0].text.as_deref() == Some("loan_status"))
            .expect("the row access");

        assert_eq!(
            row.receiver,
            Some(Binding::TableRow {
                doctype: "Loan".to_string(),
                fieldname: "items".to_string(),
            })
        );
        assert_eq!(row.slots[0].anchor, Anchor::Unknown);
    }

    #[test]
    fn the_dict_form_of_get_doc_anchors_on_its_doctype_key() {
        let found = python("frappe.get_doc({\"doctype\": \"Loan\", \"status\": \"Open\"})\n");
        let call = of(&found, "frappe.get_doc{}")[0];

        assert_eq!(call.anchor, doctype("Loan"));
        assert_eq!(fields(call), [("status", &field("Loan", "status"))]);
    }

    #[test]
    fn the_whitelist_decorator_registers_and_other_decorators_do_not() {
        let found = python("@frappe.whitelist()\ndef ping():\n\tpass\n");
        assert_eq!(of(&found, "frappe.whitelist").len(), 1);

        let aliased = python("from frappe import whitelist as w\n\n@w()\ndef ping():\n\tpass\n");
        assert_eq!(of(&aliased, "frappe.whitelist").len(), 1);

        let other = python("@functools.cache\ndef ping():\n\tpass\n");
        assert!(of(&other, "frappe.whitelist").is_empty());
    }

    #[test]
    fn import_frappe_as_f_is_not_recognized() {
        let found = python("import frappe as f\n\nf.get_doc(\"Library Member\", name)\n");
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_bare_javascript_object_key_is_a_fieldname_and_a_python_one_is_a_variable() {
        let javascript = javascript(
            "frappe.db.get_list(\"Task\", { fields: [\"subject\"], filters: { status: \"Open\" } });\n",
        );
        assert_eq!(
            fields(of(&javascript, "frappe.db.get_list")[0]),
            [
                ("subject", &field("Task", "subject")),
                ("status", &field("Task", "status")),
            ]
        );

        // `{key: 1}` in Python names a variable, not a field.
        let python = python("frappe.get_all(\"Loan\", filters={key: 1})\n");
        let call = of(&python, "frappe.get_all")[0];
        let filter = &call.slots[1];
        assert_eq!(filter.role, Role::Field);
        assert_eq!(filter.text, None);
        assert_eq!(filter.anchor, Anchor::Unknown);
    }

    #[test]
    fn add_child_dict_keys_are_child_fields_that_anchor_to_nothing() {
        let found = javascript(
            "frappe.ui.form.on(\"Task\", {\n\
             \trefresh(frm) {\n\
             \t\tfrm.add_child(\"items\", { item_code: \"x\" });\n\
             \t},\n\
             });\n",
        );
        let call = of(&found, "frm.add_child")[0];

        assert_eq!(call.receiver, form("Task"));
        assert_eq!(call.slots[0].role, Role::Field);
        assert_eq!(call.slots[1].role, Role::ChildField);
        assert_eq!(call.slots[1].text.as_deref(), Some("item_code"));
        assert_eq!(call.slots[1].anchor, Anchor::Unknown);
    }

    #[test]
    fn hook_keys_and_dotted_paths_carry_their_own_roles() {
        let found =
            python("frappe.get_hooks(\"doc_events\")\nfrappe.enqueue(\"library.api.ping\")\n");

        assert_eq!(
            of(&found, "frappe.get_hooks")[0].slots[0].role,
            Role::HookKey
        );
        assert_eq!(
            of(&found, "frappe.enqueue")[0].slots[0].anchor,
            Anchor::DottedPath("library.api.ping".to_string())
        );
    }

    #[test]
    fn a_translation_needs_a_literal_and_says_how_it_was_written() {
        let found = python("_(\"Loan created\")\n_(f\"Loan {name}\")\n");

        assert_eq!(
            of(&found, "_")[0].slots[0].role,
            Role::Translation(TranslationArg::Literal)
        );
        assert_eq!(
            of(&found, "_")[1].slots[0].role,
            Role::Translation(TranslationArg::Interpolated)
        );
    }

    #[test]
    fn form_on_anchors_its_keys_in_both_the_object_and_the_legacy_form() {
        let found = javascript(
            "frappe.ui.form.on(\"Task\", {\n\
             \trefresh(frm) {},\n\
             \t\"subject\": function (frm) {},\n\
             });\n",
        );
        let call = of(&found, "frappe.ui.form.on")[0];

        assert_eq!(call.anchor, doctype("Task"));
        let events: Vec<&str> = call
            .slots
            .iter()
            .filter(|slot| slot.role == Role::FormEvent)
            .filter_map(|slot| slot.text.as_deref())
            .collect();
        assert_eq!(events, ["refresh", "subject"]);

        // The legacy middle argument is the same namespace as an object key,
        // not a fieldname: `("Payment Request", "refresh", fn)` is a lifecycle
        // event, and reading it as a field called it missing (specs/11).
        let legacy = javascript("frappe.ui.form.on(\"Task\", \"subject\", function (frm) {});\n");
        let call = of(&legacy, "frappe.ui.form.on")[0];
        assert_eq!(call.anchor, doctype("Task"));
        let events: Vec<&str> = call
            .slots
            .iter()
            .filter(|slot| slot.role == Role::FormEvent)
            .filter_map(|slot| slot.text.as_deref())
            .collect();
        assert_eq!(events, ["subject"]);
        assert!(fields(call).is_empty());
    }

    #[test]
    fn a_form_on_call_with_a_dynamic_doctype_anchors_to_nothing() {
        let found = javascript("frappe.ui.form.on(doctype, { refresh(frm) {} });\n");
        let call = of(&found, "frappe.ui.form.on")[0];

        assert_eq!(call.anchor, Anchor::Unknown);
    }

    #[test]
    fn frm_is_bound_to_the_enclosing_form_on_and_nowhere_else() {
        let inside = javascript(
            "frappe.ui.form.on(\"Task\", {\n\
             \trefresh(frm) {\n\
             \t\tfrm.set_value(\"subject\", 1);\n\
             \t\tfrm.doc.status;\n\
             \t},\n\
             });\n",
        );
        // Bound, not anchored: `Task` could be a child DocType, whose block
        // runs on its parent's form, and only the resolver can tell.
        let set_value = of(&inside, "frm.set_value")[0];
        assert_eq!(set_value.receiver, form("Task"));
        assert_eq!(set_value.slots[0].anchor, Anchor::Unknown);
        assert_eq!(of(&inside, "frm.doc.attribute")[0].receiver, form("Task"));

        let outside = javascript("function helper(frm) {\n\tfrm.set_value(\"subject\", 1);\n}\n");
        assert!(outside.is_empty(), "{outside:?}");
    }

    #[test]
    fn set_query_puts_the_field_on_the_child_only_in_the_three_argument_form() {
        let found = javascript(
            "frappe.ui.form.on(\"Task\", {\n\
             \trefresh(frm) {\n\
             \t\tfrm.set_query(\"project\", () => {});\n\
             \t\tfrm.set_query(\"item_code\", \"items\", () => {});\n\
             \t},\n\
             });\n",
        );
        let calls = of(&found, "frm.set_query");

        assert_eq!(calls[0].receiver, form("Task"));
        assert_eq!(calls[0].slots[0].role, Role::Field);

        assert_eq!(calls[1].slots[0].role, Role::ChildField);
        assert_eq!(calls[1].slots[1].role, Role::Field);
    }

    #[test]
    fn frappe_call_takes_a_path_and_frm_call_a_controller_method() {
        let found = javascript(
            "frappe.ui.form.on(\"Task\", {\n\
             \trefresh(frm) {\n\
             \t\tfrappe.call({ method: \"library.api.ping\", args: {} });\n\
             \t\tfrm.call({ method: \"recalculate\" });\n\
             \t},\n\
             });\n",
        );

        assert_eq!(
            of(&found, "frappe.call")[0].slots[0].role,
            Role::Path(PathKind::Whitelisted)
        );
        assert_eq!(of(&found, "frm.call")[0].slots[0].role, Role::Method);
        assert_eq!(of(&found, "frm.call")[0].receiver, form("Task"));
    }

    #[test]
    fn a_dynamic_call_method_anchors_to_nothing() {
        let found = javascript("frappe.call({ method: path, args: {} });\n");
        let call = of(&found, "frappe.call")[0];

        assert_eq!(call.slots[0].anchor, Anchor::Unknown);
        assert_eq!(call.slots[0].text, None);
    }

    #[test]
    fn set_route_names_a_doctype_only_on_the_form_route() {
        let form = javascript("frappe.set_route(\"Form\", \"Task\", name);\n");
        assert_eq!(of(&form, "frappe.set_route")[0].anchor, doctype("Task"));

        let list = javascript("frappe.set_route(\"List\", \"Task\");\n");
        assert!(list.is_empty(), "{list:?}");
    }

    #[test]
    fn listview_settings_and_locals_name_a_doctype() {
        let settings =
            javascript("frappe.listview_settings[\"Task\"] = { hide_name_column: true };\n");
        assert_eq!(
            of(&settings, "frappe.listview_settings[]")[0].anchor,
            doctype("Task")
        );

        let locals = javascript("const doc = locals[\"Task\"][name];\n");
        assert_eq!(of(&locals, "locals[]")[0].anchor, doctype("Task"));

        let dynamic = javascript("const doc = locals[cdt][cdn];\n");
        assert_eq!(of(&dynamic, "locals[]")[0].anchor, Anchor::Unknown);
    }

    #[test]
    fn json_buffers_carry_no_call_shapes() {
        let found = matched(Language::Json, "{\"doctype\": \"DocType\"}\n", None);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_slot_can_be_found_by_the_offset_the_cursor_is_at() {
        let source = "frappe.get_all(\"Loan\", fields=[\"status\"])\n";
        let found = python(source);
        let call = of(&found, "frappe.get_all")[0];
        let offset = source.find("status").expect("the fieldname");

        assert_eq!(
            call.slot_at(offset).map(|slot| &slot.anchor),
            Some(&field("Loan", "status"))
        );
        assert!(call.slot_at(source.len() - 1).is_none());
    }
}
