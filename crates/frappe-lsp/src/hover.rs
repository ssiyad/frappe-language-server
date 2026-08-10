//! `textDocument/hover` — [22](../../../docs/specs/22-hover.md).
//!
//! The answer would otherwise mean opening a JSON file and reading past 200
//! fields. Never more than ~15 lines: an editor renders this in a popup, and
//! everything below the fold is the same as absent.

use frappe_index::doctype::Options;
use frappe_index::resolve::Target as Resolved;
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
        Resolved::UnknownDocType { .. }
        | Resolved::UnknownField { .. }
        | Resolved::Unresolvable => None,
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
