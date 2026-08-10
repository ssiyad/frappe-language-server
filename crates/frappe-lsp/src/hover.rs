//! `textDocument/hover` — [22](../../../docs/specs/22-hover.md).
//!
//! The answer would otherwise mean opening a JSON file and reading past 200
//! fields. Never more than ~15 lines: an editor renders this in a popup, and
//! everything below the fold is the same as absent.

use frappe_index::doctype::Options;
use frappe_index::resolve::{FormEventKind, Target as Resolved};
use frappe_index::{DocType, Index, Layer};

use crate::analysis::Found;

/// The hover body, as Markdown. `None` where nothing resolved — spec 22 is
/// explicit that a guess is worse than silence.
#[must_use]
pub fn markdown(index: &Index, found: &Found) -> Option<String> {
    match &found.target {
        Resolved::DocType { name } => doctype(index, index.doctype(name)?),
        Resolved::Field {
            doctype,
            fieldname,
            implicit,
        } => field(index, doctype, fieldname, *implicit),
        Resolved::Method { doctype, name } => method(index, doctype, name),
        Resolved::Path(resolution) => path(index, resolution),
        Resolved::DocumentMember { name } => Some(format!(
            "**{name}** — `Document` attribute\n\nDeclared by the framework, not by this DocType."
        )),
        Resolved::FormEvent {
            name,
            doctype,
            kind,
        } => Some(form_event(*kind, name, doctype)),
        Resolved::UnknownDocType { .. }
        | Resolved::UnknownField { .. }
        | Resolved::Unresolvable => None,
    }
}

/// A form event key, and which of spec 11's two namespaces it came from.
fn form_event(kind: FormEventKind, name: &str, doctype: &str) -> String {
    match kind {
        FormEventKind::Lifecycle => {
            format!("**{name}** — form lifecycle event\n\nFires on the {doctype} form.")
        }
        FormEventKind::Field => format!(
            "**{name}** — field handler\n\nFires when `{name}` changes on the {doctype} form."
        ),
        FormEventKind::TableEvent => format!(
            "**{name}** — table row event\n\nFires when a row of the table is added or removed."
        ),
        // The classic typo'd handler: valid JavaScript, never called, and
        // invisible to every other tool (spec 11).
        FormEventKind::Unknown => format!(
            "**{name}**\n\nNeither an event nor a fieldname on {doctype}. This handler never fires."
        ),
    }
}

fn doctype(index: &Index, doctype: &DocType) -> Option<String> {
    let mut out = String::new();

    let module = doctype.module.as_deref().unwrap_or("—");
    out.push_str(&format!(
        "**{}** — {module} · {}\n\n",
        doctype.name, doctype.app
    ));

    // Flags only when true: a list of "no" tells the reader nothing.
    let mut facts = Vec::new();
    if doctype.is_submittable {
        facts.push("Submittable".to_string());
    }
    if doctype.is_single {
        facts.push("Single".to_string());
    }
    if doctype.is_child {
        facts.push("Child Table".to_string());
    }
    if doctype.is_virtual {
        facts.push("Virtual".to_string());
    }
    if doctype.is_tree {
        facts.push("Tree".to_string());
    }

    let custom = doctype
        .fields
        .iter()
        .filter(|field| field.provenance.layer != Layer::Schema)
        .count();
    facts.push(if custom > 0 {
        format!("{} fields (+{custom} custom)", doctype.fields.len())
    } else {
        format!("{} fields", doctype.fields.len())
    });
    if let Some(autoname) = &doctype.autoname {
        facts.push(format!("naming: `{autoname}`"));
    }
    out.push_str(&facts.join(" · "));
    out.push('\n');

    if let Some(title) = &doctype.title_field {
        out.push_str(&format!("Title field: `{title}`\n"));
    }

    out.push_str(&format!("\n`{}`", workspace_relative(index, &doctype.path)));
    Some(out)
}

fn field(index: &Index, doctype: &str, fieldname: &str, implicit: bool) -> Option<String> {
    if implicit {
        return Some(format!(
            "**{fieldname}** — implicit field\n\nEvery document carries it; it is not in `{doctype}`'s schema."
        ));
    }

    let field = index.field(doctype, fieldname)?;
    let fieldtype = index.fieldtype(field);
    let mut out = String::new();

    // Headline carries the one thing a reader always wants: what it points at.
    match index.options(field) {
        Options::LinkTarget(target) | Options::ChildDocType(target) => {
            out.push_str(&format!("**{fieldname}** — {fieldtype} → {target}\n\n"));
        }
        Options::DynamicSource(source) => {
            out.push_str(&format!(
                "**{fieldname}** — {fieldtype}, target from `{source}`\n\n"
            ));
        }
        _ => out.push_str(&format!("**{fieldname}** — {fieldtype}\n\n")),
    }

    let mut facts = Vec::new();
    if field.reqd {
        facts.push("Required");
    }
    if field.read_only {
        facts.push("Read only");
    }
    if field.unique {
        facts.push("Unique");
    }
    if field.hidden {
        facts.push("Hidden");
    }
    if !facts.is_empty() {
        out.push_str(&facts.join(" · "));
        out.push('\n');
    }

    if let Options::Select { choices, .. } = index.options(field) {
        let shown: Vec<&str> = choices.iter().take(8).copied().collect();
        let ellipsis = if choices.len() > 8 { " …" } else { "" };
        out.push_str(&format!("Options: {}{ellipsis}\n", shown.join(", ")));
    }
    if let Some(fetch) = &field.fetch_from {
        out.push_str(&format!("Fetches from `{fetch}`\n"));
    }
    if let Some(depends) = &field.depends_on {
        out.push_str(&format!("Depends on `{depends}`\n"));
    }
    if let Some(default) = &field.default {
        out.push_str(&format!("Default `{default}`\n"));
    }

    // Provenance is stated for anything the codebase does not contain, so the
    // reader knows why grep will not find it (spec 22).
    out.push('\n');
    match field.provenance.layer {
        Layer::Schema => out.push_str(&format!("{doctype} · {}", field.provenance.app)),
        Layer::Fixture => out.push_str(&format!(
            "Custom Field · {} · fixture",
            field.provenance.app
        )),
        Layer::Site => out.push_str("Custom Field · live site"),
    }

    Some(out)
}

fn method(index: &Index, doctype: &str, name: &str) -> Option<String> {
    let controller = index.doctype(doctype)?.controller.as_ref()?;
    let method = controller.methods.iter().find(|m| m.name == name)?;

    let parameters = method
        .parameters
        .iter()
        .filter(|parameter| *parameter != "self")
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");

    let mut out = format!("**{}.{}**({parameters})\n\n", controller.class_name, name);
    if method.whitelisted {
        out.push_str("Whitelisted\n");
    }
    out.push_str(&format!("{doctype} · controller method"));
    Some(out)
}

fn path(index: &Index, resolution: &frappe_index::Resolution) -> Option<String> {
    let candidate = resolution.best()?;

    let mut out = format!("**{}**", resolution.dotted);
    if let Some(whitelisted) = index.whitelisted(&resolution.dotted) {
        let parameters = whitelisted.parameters.join(", ");
        out = format!("**{}**({parameters})\n\n", resolution.dotted);
        out.push_str("Whitelisted");
        if !whitelisted.methods.is_empty() {
            out.push_str(&format!(" · {}", whitelisted.methods.join(", ")));
        }
        out.push('\n');
        out.push_str(&format!(
            "Guest access: {}\n",
            if whitelisted.allow_guest { "yes" } else { "no" }
        ));
    } else {
        out.push_str("\n\n");
    }

    out.push_str(&format!(
        "\n`{}`",
        workspace_relative(index, &candidate.path)
    ));
    Some(out)
}

/// Paths are shown relative to the bench: the absolute prefix is the same for
/// every line and crowds out what differs.
fn workspace_relative(index: &Index, path: &std::path::Path) -> String {
    index
        .workspace()
        .detection
        .bench_root()
        .and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Markdown reads acceptably as plain text, but the emphasis markers are noise
/// in a client that will not render them ([30](../../../docs/specs/30-configuration.md)).
#[must_use]
pub fn as_plain_text(markdown: &str) -> String {
    markdown.replace("**", "").replace('`', "")
}

/// Hover for a `hooks.py` position.
///
/// The value here is orientation: `doc_events` ordering is the single most
/// common source of confusion, and the answer is otherwise a documentation
/// page ([22](../../../docs/specs/22-hover.md)).
#[must_use]
pub fn hook_markdown(index: &Index, target: &crate::hooks::HookTarget) -> Option<String> {
    use crate::hooks::HookTarget;

    Some(match target {
        HookTarget::Key { name, schema } => match schema {
            Some(schema) => format!(
                "**{name}** — `{schema:?}`\n\n{}\n\n[Hooks documentation](https://docs.frappe.io/framework/user/en/basics/hooks)",
                describe(*schema)
            ),
            // Reported rather than guessed at: a name frappe never reads does
            // nothing, silently, which is the whole point of the check.
            None => format!("**{name}**\n\nNot a hook `frappe` reads. Setting it has no effect."),
        },

        HookTarget::DocType(name) => {
            if name == "*" {
                "**\\*** — every DocType\n\nHandlers registered here run for every document type."
                    .to_string()
            } else {
                return doctype_summary(index, name);
            }
        }

        HookTarget::DocEvent {
            name,
            known,
            doctype,
        } => {
            let mut out = if *known {
                format!(
                    "**{name}** — controller lifecycle event\n\n{}\n",
                    when(name)
                )
            } else {
                format!("**{name}**\n\nNot an event `frappe` fires; the handler never runs.\n")
            };
            // Registering a submit event on a DocType that cannot be submitted
            // is dead code — spec 12.
            if let Some(doctype) = doctype
                && frappe_syntax::generated::is_submit_only(name)
                && index.doctype(doctype).is_some_and(|d| !d.is_submittable)
            {
                out.push_str(&format!(
                    "\n`{doctype}` is not submittable, so this never fires.\n"
                ));
            }
            out
        }

        HookTarget::Frequency { name, known } => {
            if *known {
                format!("**{name}** — scheduler frequency")
            } else {
                format!("**{name}**\n\nNot a frequency the scheduler knows; nothing runs.")
            }
        }

        HookTarget::Cron { expression, valid } => {
            if *valid {
                format!("`{expression}` — cron expression")
            } else {
                format!(
                    "`{expression}`\n\nNot a valid 5-field cron expression. The scheduler raises on this at sync time, far from here."
                )
            }
        }

        HookTarget::Path { dotted, kind } => {
            let effective = index.effective_method(dotted);
            let mut out = format!("**{dotted}** — {kind:?}");
            if effective != dotted {
                out.push_str(&format!("\n\nRemapped to `{effective}`."));
            }
            match index.resolve(effective, *kind).best() {
                Some(candidate) => {
                    out.push_str(&format!("\n\n`{}`", candidate.path.display()));
                }
                None => out.push_str("\n\nResolves to nothing in this workspace."),
            }
            out
        }

        HookTarget::Text(_) => return None,

        HookTarget::App(name) => match index.apps().iter().find(|app| app.name == *name) {
            Some(app) => format!("**{name}** — installed\n\n`{}`", app.package.display()),
            None => format!("**{name}**\n\nNot installed in this bench."),
        },

        HookTarget::Asset { written, file } => match file {
            Some(path) => format!("**{written}**\n\n`{}`", path.display()),
            None => format!("**{written}**\n\nNo such file in this app."),
        },
    })
}

fn doctype_summary(index: &Index, name: &str) -> Option<String> {
    doctype(index, index.doctype(name)?)
}

fn describe(schema: frappe_syntax::generated::Schema) -> &'static str {
    use frappe_syntax::generated::Schema;
    match schema {
        Schema::Callable => "A dotted path to a function.",
        Schema::File => "A path to an asset in this app.",
        Schema::DocEvents => "DocType, then event, then the handler to run.",
        Schema::MapToClass => "DocType to the class that replaces its controller.",
        Schema::MapToClasses => "DocType to the classes mixed into its controller.",
        Schema::MapToCallable => "DocType to a function.",
        Schema::MapToFile => "DocType to a client script in this app.",
        Schema::MapPathToPath => "A whitelisted path to the one that replaces it.",
        Schema::Scheduler => "Frequency to the functions run at it; `cron` takes expressions.",
        Schema::Fixtures => "DocTypes exported with the app.",
        Schema::Jinja => "Modules and functions exposed to templates.",
        Schema::Routes => "Website routes.",
        Schema::AppNames => "Apps this one requires.",
        Schema::Unknown => "Read by frappe; nothing here validates its value.",
    }
}

/// Where an event sits relative to its neighbours — spec 22 calls the ordering
/// of `validate` / `before_save` / `on_update` the most common confusion.
fn when(event: &str) -> &'static str {
    match event {
        "before_validate" => "Before `validate`, on every save.",
        "validate" => "On every save, before the document is written.",
        "before_save" => "After `validate`, immediately before the write.",
        "autoname" | "before_naming" => "While the name is being decided, before the first insert.",
        "before_insert" => "Before the first write only.",
        "after_insert" => "After the first write only.",
        "on_update" => "After every write, including the first.",
        "before_submit" => "Before docstatus moves to 1.",
        "on_submit" => "After docstatus moves to 1.",
        "before_cancel" => "Before docstatus moves to 2.",
        "on_cancel" => "After docstatus moves to 2.",
        "on_update_after_submit" => "After a write to a submitted document.",
        "on_change" => "After any write, including `db_set`.",
        "on_trash" => "Before the row is deleted.",
        "after_delete" => "After the row is deleted.",
        _ => "See the hooks documentation for when this fires.",
    }
}
