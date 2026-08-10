//! The metadata graph — [04](../../../docs/specs/04-metadata-index.md) end to
//! end: parse, layer merge, conflicts, controllers, the whitelist registry and
//! the dotted-path resolver over the result.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::app::App;
use crate::controller;
use crate::doctype::{self, DocType, Field, Layer, Options, Provenance, Raw};
use crate::fieldtypes;
use crate::fixtures::{self, Fixture};
use crate::intern::{Interner, Symbol};
use crate::paths::{self, Kind, Resolution, Resolver};
use crate::problem::Problem;
use crate::span::Range;
use crate::walk::Walk;
use crate::whitelist::{self, WhitelistedMethod};
use crate::workspace::Workspace;
use frappe_syntax::GrammarError;
use frappe_syntax::symbols as python;

/// How complete the index is — the gate on absence checks in
/// [23](../../../docs/specs/23-diagnostics.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Confidence {
    /// Bridge enabled and synced. Not reachable until 014.
    Full,
    /// Layers 1 and 2 only, no bridge configured.
    Static,
    /// Bridge configured but unreachable. Not reachable until 014.
    Degraded,
    /// The initial build has not finished.
    Partial,
}

impl Confidence {
    /// The name `frappe-lsp/indexStatus` reports.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Static => "static",
            Self::Degraded => "degraded",
            Self::Partial => "partial",
        }
    }
}

/// What `frappe-lsp/indexStatus` answers with
/// ([02](../../../docs/specs/02-architecture.md)).
///
/// The protocol method lives in the binary crate; this is the value it
/// serializes, so integration tests can assert against the index without
/// reaching inside the process ([32](../../../docs/specs/32-testing.md)).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IndexStatus {
    /// How much of the picture the index has.
    pub confidence: Confidence,
    /// Installed apps.
    pub apps: usize,
    /// DocTypes after the layer merge.
    pub doctypes: usize,
    /// Declared fields across every DocType; implicit fields are not counted.
    pub fields: usize,
    /// DocTypes with a controller class.
    pub controllers: usize,
    /// Entries in the whitelist registry.
    pub whitelisted_methods: usize,
    /// Python modules read.
    pub modules: usize,
    /// Merge conflicts ([`Conflict`]).
    pub conflicts: usize,
    /// Files the index could not use.
    pub problems: usize,
    /// Detected Frappe version, or `None` when every version-gated check must
    /// stay silent ([31](../../../docs/specs/31-performance.md)).
    pub frappe_version: Option<String>,
    /// How long the last full build took.
    pub build_time: Duration,
}

/// A controller class contributed by `hooks.py` rather than by sitting beside
/// the DocType JSON.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ControllerExtra {
    /// Whether it replaces the base controller or extends it.
    pub kind: ExtraKind,
    /// The dotted path as written in `hooks.py`.
    pub dotted: String,
    /// The file holding the class.
    pub path: PathBuf,
    /// The class name.
    pub class_name: String,
    /// The class statement and its body.
    pub range: Range,
    /// The class name alone.
    pub name_range: Range,
    /// Methods it declares, which join the DocType's method set (spec 04).
    pub methods: Vec<String>,
}

/// How a hook contributes a class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExtraKind {
    /// `override_doctype_class`: replaces the base controller.
    Override,
    /// `extend_doctype_class`: adds a mixin.
    Mixin,
}

/// A merge conflict. Both codes are from
/// [23](../../../docs/specs/23-diagnostics.md).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum ConflictCode {
    /// The same DocType defined by two installed apps.
    DuplicateDocType,
    /// A Custom Field colliding with a schema field.
    CustomFieldCollision,
}

impl ConflictCode {
    /// The diagnostic code, without the `frappe-lsp/` prefix.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateDocType => "duplicate-doctype",
            Self::CustomFieldCollision => "custom-field-collision",
        }
    }
}

/// One conflict, carrying every file it should be reported against.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct Conflict {
    /// The code to publish it under.
    pub code: ConflictCode,
    /// The DocType involved.
    pub doctype: String,
    /// The field, for a Custom Field collision.
    pub fieldname: Option<String>,
    /// Every file the diagnostic lands on — both sides of a duplicate.
    pub paths: Vec<PathBuf>,
    /// The range inside the last path, when there is one.
    pub range: Option<Range>,
}

/// A DocType JSON queued for parsing: app, file, package and install order.
type Queued = (Arc<str>, Arc<Path>, Arc<Path>, usize);

/// A queued DocType JSON with its parse result.
type Parsed = (Arc<str>, Arc<Path>, Arc<Path>, usize, Result<Raw, String>);

/// A fixtures file with its parse result.
type ParsedFixture = (usize, Arc<str>, PathBuf, Result<Fixture, String>);

/// One Layer-2 file, with the app position that breaks ties inside the layer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct LayerTwo {
    order: usize,
    app: Arc<str>,
    fixture: Fixture,
}

/// One Layer-1 file contributing a DocType name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
struct Source {
    order: usize,
    path: Arc<Path>,
    app: Arc<str>,
    package: Arc<Path>,
}

/// The resolved graph every feature queries.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Index {
    workspace: Workspace,
    interner: Interner,
    doctypes: BTreeMap<String, DocType>,
    sources: BTreeMap<String, Vec<Source>>,
    fixtures: BTreeMap<PathBuf, LayerTwo>,
    modules: BTreeMap<PathBuf, python::Module>,
    packages: Vec<(String, PathBuf)>,
    whitelisted: BTreeMap<String, WhitelistedMethod>,
    hooks: BTreeMap<String, crate::hooks::Hooks>,
    controller_extras: BTreeMap<String, Vec<ControllerExtra>>,
    remapped: BTreeMap<String, String>,
    conflicts: Vec<Conflict>,
    problems: Vec<Problem>,
    confidence: Confidence,
    build_time: Duration,
}

impl Index {
    /// Discover and index the workspace containing `root`.
    #[must_use]
    pub fn build(root: &Path) -> Self {
        Self::from_workspace(Workspace::discover(root))
    }

    /// An empty index, for the window before the first build completes.
    ///
    /// `initialize` MUST NOT block on the build
    /// ([31](../../../docs/specs/31-performance.md)), so the server needs
    /// something to answer with that reports its own incompleteness.
    #[must_use]
    pub fn partial() -> Self {
        Self {
            // Built by hand rather than by discovery: this value exists before
            // a root is known, and touching the filesystem for it would be a
            // syscall spent answering "nothing yet".
            workspace: Workspace {
                detection: crate::detect::Detection {
                    mode: crate::detect::Mode::None,
                    root: PathBuf::new(),
                    bench: None,
                },
                apps: Vec::new(),
                excluded: Vec::new(),
                sites: Vec::new(),
                current_site: None,
                problems: Vec::new(),
            },
            interner: Interner::new(),
            doctypes: BTreeMap::new(),
            sources: BTreeMap::new(),
            fixtures: BTreeMap::new(),
            modules: BTreeMap::new(),
            packages: Vec::new(),
            whitelisted: BTreeMap::new(),
            conflicts: Vec::new(),
            problems: Vec::new(),
            hooks: BTreeMap::new(),
            controller_extras: BTreeMap::new(),
            remapped: BTreeMap::new(),
            confidence: Confidence::Partial,
            build_time: Duration::ZERO,
        }
    }

    /// Index an already-discovered workspace.
    #[must_use]
    pub fn from_workspace(workspace: Workspace) -> Self {
        let started = Instant::now();

        let packages = workspace
            .apps
            .iter()
            .filter_map(|app| {
                Some((
                    app.package.file_name()?.to_str()?.to_string(),
                    app.package.clone(),
                ))
            })
            .collect();

        let mut index = Self {
            // Discovery's problems are already diagnostics; the index adds to
            // them rather than starting a second list.
            problems: workspace.problems.clone(),
            hooks: BTreeMap::new(),
            controller_extras: BTreeMap::new(),
            remapped: BTreeMap::new(),
            workspace,
            interner: Interner::new(),
            doctypes: BTreeMap::new(),
            sources: BTreeMap::new(),
            fixtures: BTreeMap::new(),
            modules: BTreeMap::new(),
            packages,
            whitelisted: BTreeMap::new(),
            conflicts: Vec::new(),
            confidence: Confidence::Static,
            build_time: Duration::ZERO,
        };

        // DocType JSON parsing is embarrassingly parallel; the merge below is
        // not ([31](../../../docs/specs/31-performance.md)).
        let mut raws = index.parse_doctypes();
        index.read_fixtures();
        index.read_python();

        let names: Vec<String> = index.sources.keys().cloned().collect();
        for name in names {
            index.assemble(&name, &mut raws);
        }
        index.rebuild_whitelist();
        // After the controllers and the whitelist registry exist, because it
        // alters both: hooks feed the index rather than only being validated
        // ([12](../../../docs/specs/12-surface-hooks.md)).
        index.apply_hooks();
        index.sort_reports();

        index.build_time = started.elapsed();
        index
    }

    /// Every DocType JSON, read and parsed in parallel.
    fn parse_doctypes(&mut self) -> BTreeMap<PathBuf, Raw> {
        let files: Vec<Queued> = self
            .workspace
            .apps
            .iter()
            .flat_map(|app| {
                let name: Arc<str> = Arc::from(app.name.as_str());
                let package: Arc<Path> = Arc::from(app.package.clone());
                app.artifacts.doctypes().map(move |record| {
                    (
                        Arc::clone(&name),
                        Arc::from(record.path.clone()),
                        Arc::clone(&package),
                        app.order,
                    )
                })
            })
            .collect();

        let parsed: Vec<Parsed> = files
            .into_par_iter()
            .map(|(app, path, package, order)| {
                let raw = read_and_parse(&path);
                (app, path, package, order, raw)
            })
            .collect();

        let mut raws = BTreeMap::new();
        for (app, path, package, order, raw) in parsed {
            match raw {
                Ok(raw) => {
                    self.sources
                        .entry(raw.name.clone())
                        .or_default()
                        .push(Source {
                            order,
                            path: Arc::clone(&path),
                            app,
                            package,
                        });
                    raws.insert(path.to_path_buf(), raw);
                }
                Err(message) => self.problems.push(Problem::new(&path, message)),
            }
        }
        for sources in self.sources.values_mut() {
            sources.sort();
        }
        raws
    }

    /// Every `<app>/fixtures/*.json` — Layer 2.
    fn read_fixtures(&mut self) {
        let files: Vec<(usize, Arc<str>, PathBuf)> = self
            .workspace
            .apps
            .iter()
            .flat_map(|app| {
                let name: Arc<str> = Arc::from(app.name.as_str());
                app.artifacts
                    .fixtures
                    .iter()
                    .map(move |path| (app.order, Arc::clone(&name), path.clone()))
            })
            .collect();

        let read: Vec<ParsedFixture> = files
            .into_par_iter()
            .map(|(order, app, path)| {
                let fixture = std::fs::read_to_string(&path)
                    .map_err(|error| format!("unreadable: {error}"))
                    .and_then(|source| fixtures::read(&source));
                (order, app, path, fixture)
            })
            .collect();

        for (order, app, path, fixture) in read {
            match fixture {
                Ok(fixture) => {
                    self.fixtures.insert(
                        path,
                        LayerTwo {
                            order,
                            app,
                            fixture,
                        },
                    );
                }
                Err(message) => self.problems.push(Problem::new(&path, message)),
            }
        }
    }

    /// Every `.py` under an app package.
    fn read_python(&mut self) {
        let files: Vec<PathBuf> = self
            .workspace
            .apps
            .iter()
            .flat_map(|app| {
                Walk::new(&app.package)
                    .filter(|path| path.extension().is_some_and(|extension| extension == "py"))
            })
            .collect();

        let read: Vec<(PathBuf, Result<python::Module, GrammarError>)> = files
            .into_par_iter()
            .filter_map(|path| {
                let source = std::fs::read_to_string(&path).ok()?;
                // A grammar that will not load is a build fault, not a source
                // one: it yields no symbols for every file alike, and the
                // problem below says so once per file rather than silently.
                Some((path, python::read(&source)))
            })
            .collect();

        for (path, module) in read {
            match module {
                Ok(module) => {
                    self.modules.insert(path, module);
                }
                Err(error) => self.problems.push(Problem::new(path, error.to_string())),
            }
        }
    }

    /// Build one DocType from every layer that contributes to it.
    ///
    /// The merge is per-name: nothing about `Sales Invoice` depends on any
    /// other DocType, which is what makes the incremental path in
    /// [`Index::reindex`] equal to a full rebuild rather than merely close.
    fn assemble(&mut self, name: &str, raws: &mut BTreeMap<PathBuf, Raw>) {
        self.doctypes.remove(name);
        self.conflicts.retain(|conflict| conflict.doctype != name);

        let Some(sources) = self.sources.get(name).filter(|s| !s.is_empty()) else {
            self.sources.remove(name);
            return;
        };

        // Later app wins; both files still get the diagnostic.
        if sources.len() > 1 {
            self.conflicts.push(Conflict {
                code: ConflictCode::DuplicateDocType,
                doctype: name.to_string(),
                fieldname: None,
                paths: sources.iter().map(|s| s.path.to_path_buf()).collect(),
                range: None,
            });
        }

        let winner = sources.last().cloned().expect("checked non-empty above");
        let raw = match raws.remove(winner.path.as_ref()) {
            Some(raw) => raw,
            // Only the incremental path gets here: a rebuild triggered by a
            // neighbouring file has to re-read the winner from disk.
            None => match read_and_parse(&winner.path) {
                Ok(raw) => raw,
                Err(_) => return,
            },
        };

        let provenance = Provenance {
            layer: Layer::Schema,
            app: Arc::clone(&winner.app),
            source: Some(Arc::clone(&winner.path)),
        };

        let mut doctype = DocType {
            name: raw.name,
            module: raw.module,
            app: Arc::clone(&winner.app),
            path: Arc::clone(&winner.path),
            is_child: raw.is_child,
            is_single: raw.is_single,
            is_submittable: raw.is_submittable,
            is_virtual: raw.is_virtual,
            is_tree: raw.is_tree,
            autoname: raw.autoname,
            naming_rule: raw.naming_rule,
            title_field: raw.title_field,
            sort_field: raw.sort_field,
            image_field: raw.image_field,
            search_fields: raw.search_fields,
            fields: raw
                .fields
                .into_iter()
                .map(|field| field.into_field(&mut self.interner, provenance.clone()))
                .collect(),
            field_order: raw
                .field_order
                .iter()
                .map(|fieldname| self.interner.intern(fieldname))
                .collect(),
            permissions: raw.permissions,
            links: raw.links,
            states: raw.states,
            controller: None,
            range: raw.range,
            meta_ranges: raw.meta_ranges,
            provenance,
        };

        self.merge_layer_two(&mut doctype);
        doctype.controller = self.controller_for(&doctype, &winner);
        self.doctypes.insert(doctype.name.clone(), doctype);
    }

    /// Apply every Custom Field and Property Setter targeting this DocType.
    fn merge_layer_two(&mut self, doctype: &mut DocType) {
        // App order breaks ties within a layer
        // ([04](../../../docs/specs/04-metadata-index.md)), so the visit order
        // is install order first and path second — not the map's path order.
        let mut files: Vec<(usize, PathBuf)> = self
            .fixtures
            .iter()
            .map(|(path, layer)| (layer.order, path.clone()))
            .collect();
        files.sort();

        let mut conflicts = Vec::new();
        for (_, path) in files {
            let Some(layer) = self.fixtures.get(&path) else {
                continue;
            };
            let (app, fixture) = (Arc::clone(&layer.app), &layer.fixture);
            let shared: Arc<Path> = Arc::from(path.clone());

            for record in &fixture.custom_fields {
                if record.doctype != doctype.name {
                    continue;
                }
                let fieldname = self.interner.intern(&record.field.fieldname);
                if doctype.field(fieldname).is_some() {
                    // A Custom Field never overrides a schema field — that is
                    // what Property Setter is for.
                    conflicts.push(Conflict {
                        code: ConflictCode::CustomFieldCollision,
                        doctype: doctype.name.clone(),
                        fieldname: Some(record.field.fieldname.clone()),
                        paths: vec![path.clone()],
                        range: Some(record.field.range),
                    });
                    continue;
                }

                let field = record.field.clone().into_field(
                    &mut self.interner,
                    Provenance {
                        layer: Layer::Fixture,
                        app: Arc::clone(&app),
                        source: Some(Arc::clone(&shared)),
                    },
                );
                let after = record
                    .insert_after
                    .as_deref()
                    .and_then(|name| self.interner.get(name));
                insert_after(&mut doctype.fields, field, after);
                insert_order(&mut doctype.field_order, fieldname, after);
            }

            for record in &fixture.property_setters {
                if record.doctype != doctype.name {
                    continue;
                }
                apply_property(&mut self.interner, doctype, record);
            }
        }

        self.conflicts.extend(conflicts);
    }

    /// The `<x>.py` beside the DocType JSON.
    fn controller_for(
        &self,
        doctype: &DocType,
        source: &Source,
    ) -> Option<controller::ControllerRef> {
        let stem = doctype.path.file_stem()?;
        let path = doctype.path.parent()?.join(stem).with_extension("py");
        let module = self.modules.get(&path)?;
        let dotted = paths::dotted_module(&source.package, &path)?;
        controller::resolve(module, &path, &dotted, &doctype.name)
    }

    /// Read every app's `hooks.py` and let it change the graph.
    ///
    /// Spec 12's cross-hook effects: a DocType's controller can be replaced or
    /// extended from another app entirely, and a whitelisted path can be
    /// remapped to a different function. Neither is visible in the file the
    /// user is reading, which is exactly why the server has to know.
    fn apply_hooks(&mut self) {
        self.hooks.clear();
        self.controller_extras.clear();
        self.remapped.clear();

        let apps: Vec<(String, PathBuf)> = self
            .workspace
            .apps
            .iter()
            .map(|app| (app.name.clone(), app.package.join("hooks.py")))
            .collect();

        for (app, path) in apps {
            if !path.is_file() {
                continue;
            }
            let hooks = crate::hooks::read(&path, &app);

            for (doctype, class) in &hooks.overrides {
                self.record_extra(doctype, class, ExtraKind::Override);
            }
            for (doctype, classes) in &hooks.mixins {
                for class in classes {
                    self.record_extra(doctype, class, ExtraKind::Mixin);
                }
            }
            for (original, effective) in &hooks.remapped {
                self.remapped.insert(original.clone(), effective.clone());
            }

            self.hooks.insert(app, hooks);
        }
    }

    /// Resolve a hook's class path to something navigable.
    fn record_extra(&mut self, doctype: &str, class: &str, kind: ExtraKind) {
        let resolution = self.resolve(class, Kind::Class);
        // `best()` falls back to a candidate of the wrong kind so a caller can
        // say *what* it found instead. Here the kind is the whole point: a
        // function is not a controller class, and treating one as a class puts
        // go-to-definition on the wrong symbol and invents methods for the
        // DocType. An unresolvable or wrong-kind path is a diagnostic
        // ([23](../../../docs/specs/23-diagnostics.md)), not a target.
        let Some(candidate) = resolution
            .candidates
            .iter()
            .find(|candidate| candidate.matches_kind)
        else {
            return;
        };
        self.controller_extras
            .entry(doctype.to_string())
            .or_default()
            .push(ControllerExtra {
                kind,
                dotted: class.to_string(),
                path: candidate.path.clone(),
                class_name: candidate.name.clone(),
                range: candidate.range,
                name_range: candidate.selection_range,
                methods: self
                    .modules
                    .get(&candidate.path)
                    .and_then(|module| module.class(&candidate.name))
                    .map(|class| class.methods.iter().map(|m| m.name.clone()).collect())
                    .unwrap_or_default(),
            });
    }

    fn rebuild_whitelist(&mut self) {
        self.whitelisted.clear();
        let packages: Vec<PathBuf> = self.packages.iter().map(|(_, path)| path.clone()).collect();

        for (path, module) in &self.modules {
            let Some(package) = packages.iter().find(|package| path.starts_with(package)) else {
                continue;
            };
            let Some(dotted) = paths::dotted_module(package, path) else {
                continue;
            };
            for method in whitelist::methods_of(module, path, &dotted) {
                self.whitelisted.insert(method.dotted.clone(), method);
            }
        }
    }

    fn sort_reports(&mut self) {
        self.conflicts.sort();
        self.conflicts.dedup();
        self.problems
            .sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.message.cmp(&b.message)));
        self.problems.dedup();
    }
}

/// Read and parse one DocType JSON, naming the file in every error.
fn read_and_parse(path: &Path) -> Result<Raw, String> {
    let source = std::fs::read_to_string(path).map_err(|error| format!("unreadable: {error}"))?;
    doctype::parse(&source).map_err(|error| format!("malformed JSON: {error}"))
}

/// Insert a Custom Field after `after`, or at the end.
fn insert_after(fields: &mut Vec<Field>, field: Field, after: Option<Symbol>) {
    let at = after
        .and_then(|target| fields.iter().position(|field| field.fieldname == target))
        .map_or(fields.len(), |position| position + 1);
    fields.insert(at, field);
}

fn insert_order(order: &mut Vec<Symbol>, fieldname: Symbol, after: Option<Symbol>) {
    let at = after
        .and_then(|target| order.iter().position(|entry| *entry == target))
        .map_or(order.len(), |position| position + 1);
    order.insert(at, fieldname);
}

/// Apply one Property Setter.
///
/// Only the properties that change what the server answers are applied. A
/// Property Setter naming anything else is ignored rather than stored: nothing
/// reads it, and pretending to have applied it would be worse than the gap.
fn apply_property(
    interner: &mut Interner,
    doctype: &mut DocType,
    record: &crate::fixtures::PropertySetter,
) {
    let value = record.value.clone();
    let truthy = value == "1" || value.eq_ignore_ascii_case("true");

    let Some(field_name) = record.field_name.as_deref() else {
        match record.property.as_str() {
            "search_fields" => doctype.search_fields = non_empty(value),
            "title_field" => doctype.title_field = non_empty(value),
            "sort_field" => doctype.sort_field = non_empty(value),
            "image_field" => doctype.image_field = non_empty(value),
            "autoname" => doctype.autoname = non_empty(value),
            "naming_rule" => doctype.naming_rule = non_empty(value),
            "is_submittable" => doctype.is_submittable = truthy,
            "istable" => doctype.is_child = truthy,
            "issingle" => doctype.is_single = truthy,
            _ => {}
        }
        return;
    };

    let Some(symbol) = interner.get(field_name) else {
        return;
    };
    let Some(field) = doctype
        .fields
        .iter_mut()
        .find(|field| field.fieldname == symbol)
    else {
        return;
    };

    match record.property.as_str() {
        "label" => field.label = non_empty(value),
        "options" => field.options = non_empty(value),
        "default" => field.default = non_empty(value),
        "depends_on" => field.depends_on = non_empty(value),
        "mandatory_depends_on" => field.mandatory_depends_on = non_empty(value),
        "read_only_depends_on" => field.read_only_depends_on = non_empty(value),
        "fetch_from" => field.fetch_from = non_empty(value),
        "fieldtype" => field.fieldtype = interner.intern(&value),
        "reqd" => field.reqd = truthy,
        "hidden" => field.hidden = truthy,
        "read_only" => field.read_only = truthy,
        "unique" => field.unique = truthy,
        "permlevel" => field.permlevel = value.parse().unwrap_or(field.permlevel),
        _ => return,
    }

    // The field is no longer purely what the schema said it was.
    field.provenance.layer = Layer::Fixture;
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

// --- Queries -----------------------------------------------------------------

impl Index {
    /// The workspace this index was built from.
    #[must_use]
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Installed apps, in install order.
    #[must_use]
    pub fn apps(&self) -> &[App] {
        &self.workspace.apps
    }

    /// A DocType by exact, case-sensitive name — the only lookup that resolves
    /// ([04](../../../docs/specs/04-metadata-index.md)).
    #[must_use]
    pub fn doctype(&self, name: &str) -> Option<&DocType> {
        self.doctypes.get(name)
    }

    /// Every DocType, by name.
    pub fn doctypes(&self) -> impl Iterator<Item = &DocType> {
        self.doctypes.values()
    }

    /// The DocType a JSON file defines.
    ///
    /// Goes through the file the merge kept, never through an unscrub of the
    /// path: `Item-Group` and `Item Group` both scrub to `item_group`, so the
    /// path cannot name the DocType on its own
    /// ([04](../../../docs/specs/04-metadata-index.md)).
    #[must_use]
    pub fn doctype_at(&self, path: &Path) -> Option<&DocType> {
        self.doctypes
            .values()
            .find(|doctype| doctype.path.as_ref() == path)
    }

    /// DocType names differing from `name` only in case.
    ///
    /// A completion suggestion or a "did you mean", never a resolution: Frappe
    /// is case-sensitive here.
    #[must_use]
    pub fn case_insensitive_matches(&self, name: &str) -> Vec<&str> {
        self.doctypes
            .keys()
            .filter(|candidate| candidate.eq_ignore_ascii_case(name) && *candidate != name)
            .map(String::as_str)
            .collect()
    }

    /// The text behind an interned symbol.
    #[must_use]
    pub fn text(&self, symbol: Symbol) -> &str {
        self.interner.resolve(symbol)
    }

    /// A field's name.
    #[must_use]
    pub fn fieldname(&self, field: &Field) -> &str {
        self.text(field.fieldname)
    }

    /// A field's type.
    #[must_use]
    pub fn fieldtype(&self, field: &Field) -> &str {
        self.text(field.fieldtype)
    }

    /// A declared field of a DocType.
    #[must_use]
    pub fn field(&self, doctype: &str, fieldname: &str) -> Option<&Field> {
        let symbol = self.interner.get(fieldname)?;
        self.doctype(doctype)?.field(symbol)
    }

    /// Whether `fieldname` exists on `doctype`, implicit fields included.
    #[must_use]
    pub fn has_field(&self, doctype: &str, fieldname: &str) -> bool {
        self.doctype(doctype)
            .is_some_and(|doctype| doctype.has_field(&self.interner, fieldname))
    }

    /// Every field name of a DocType: declared first, then the implicit fields
    /// it does not already declare.
    #[must_use]
    pub fn field_names(&self, doctype: &str) -> Vec<&str> {
        let Some(doctype) = self.doctype(doctype) else {
            return Vec::new();
        };
        let declared: Vec<&str> = doctype
            .fields
            .iter()
            .map(|field| self.fieldname(field))
            .collect();

        let mut names = declared.clone();
        for implicit in doctype.implicit_fields() {
            if !declared.contains(&implicit.fieldname) {
                names.push(implicit.fieldname);
            }
        }
        names
    }

    /// What a field's `options` means, resolved against its fieldtype.
    #[must_use]
    pub fn options<'a>(&self, field: &'a Field) -> Options<'a> {
        doctype::options_of(self.fieldtype(field), field.options.as_deref())
    }

    /// The sibling field a `Dynamic Link` names, if it exists.
    #[must_use]
    pub fn dynamic_link_source<'a>(
        &self,
        doctype: &'a DocType,
        field: &Field,
    ) -> Option<&'a Field> {
        let Options::DynamicSource(fieldname) = self.options(field) else {
            return None;
        };
        doctype.field(self.interner.get(fieldname)?)
    }

    /// The field a `fetch_from` reads, following the link half first.
    ///
    /// Returns the link field on this DocType and the target field on the
    /// DocType it points at; either half may be missing, which is what
    /// `unresolvable-fetch-from` reports on.
    #[must_use]
    pub fn fetch_from<'a>(
        &'a self,
        doctype: &'a DocType,
        field: &Field,
    ) -> (Option<&'a Field>, Option<&'a Field>) {
        let Some(parts) = field
            .fetch_from
            .as_deref()
            .and_then(doctype::split_fetch_from)
        else {
            return (None, None);
        };

        let link = self
            .interner
            .get(parts.link_fieldname)
            .and_then(|symbol| doctype.field(symbol));
        let target = link
            .and_then(|link| match self.options(link) {
                Options::LinkTarget(name) => self.doctype(name),
                _ => None,
            })
            .and_then(|target| {
                self.interner
                    .get(parts.target_fieldname)
                    .and_then(|symbol| target.field(symbol))
            });

        (link, target)
    }

    /// A whitelisted method by dotted path.
    #[must_use]
    pub fn whitelisted(&self, dotted: &str) -> Option<&WhitelistedMethod> {
        self.whitelisted.get(dotted)
    }

    /// Every whitelisted method, by dotted path.
    pub fn whitelisted_methods(&self) -> impl Iterator<Item = &WhitelistedMethod> {
        self.whitelisted.values()
    }

    /// A resolver over this workspace's packages and modules.
    #[must_use]
    pub fn resolver(&self) -> Resolver<'_> {
        Resolver {
            packages: &self.packages,
            modules: &self.modules,
        }
    }

    /// Resolve a dotted path ([07](../../../docs/specs/07-dotted-paths.md)).
    #[must_use]
    pub fn resolve(&self, dotted: &str, kind: Kind) -> Resolution {
        self.resolver().resolve(dotted, kind)
    }

    /// A parsed Python module by path, for callers that need its symbols.
    #[must_use]
    pub fn module(&self, path: &Path) -> Option<&python::Module> {
        self.modules.get(path)
    }

    /// Merge conflicts, sorted.
    #[must_use]
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// Files the index could not use, sorted.
    #[must_use]
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// How complete the index is.
    #[must_use]
    pub fn confidence(&self) -> Confidence {
        self.confidence
    }

    /// What `frappe-lsp/indexStatus` reports.
    #[must_use]
    pub fn status(&self) -> IndexStatus {
        IndexStatus {
            confidence: self.confidence,
            apps: self.workspace.apps.len(),
            doctypes: self.doctypes.len(),
            fields: self
                .doctypes
                .values()
                .map(|doctype| doctype.fields.len())
                .sum(),
            controllers: self
                .doctypes
                .values()
                .filter(|doctype| doctype.controller.is_some())
                .count(),
            whitelisted_methods: self.whitelisted.len(),
            modules: self.modules.len(),
            conflicts: self.conflicts.len(),
            problems: self.problems.len(),
            frappe_version: self.workspace.frappe_version().map(str::to_string),
            build_time: self.build_time,
        }
    }

    /// How many distinct strings interning saved storing repeatedly.
    #[must_use]
    pub fn interned(&self) -> usize {
        self.interner.len()
    }
}

// --- Incremental re-index ----------------------------------------------------

/// What a re-index did.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Reindexed {
    /// DocTypes rebuilt.
    pub doctypes: Vec<String>,
    /// Whether the file was one the index reads at all.
    pub known: bool,
}

impl Index {
    /// Classes `hooks.py` contributes to a DocType's controller, override
    /// first ([21](../../../docs/specs/21-navigation.md) offers both).
    #[must_use]
    pub fn controller_extras(&self, doctype: &str) -> &[ControllerExtra] {
        self.controller_extras
            .get(doctype)
            .map_or(&[][..], Vec::as_slice)
    }

    /// Whether a DocType's controller declares a method, counting the classes
    /// `extend_doctype_class` and `override_doctype_class` bring in.
    #[must_use]
    pub fn has_controller_method(&self, doctype: &str, name: &str) -> bool {
        let declared = self
            .doctype(doctype)
            .and_then(|doctype| doctype.controller.as_ref())
            .is_some_and(|controller| controller.methods.iter().any(|m| m.name == name));

        declared
            || self
                .controller_extras(doctype)
                .iter()
                .any(|extra| extra.methods.iter().any(|method| method == name))
    }

    /// The effective target of a whitelisted path.
    ///
    /// `override_whitelisted_methods` remaps a path, and a `frappe.call` to the
    /// original lands on the replacement at runtime — so navigation must too
    /// ([07](../../../docs/specs/07-dotted-paths.md)).
    #[must_use]
    pub fn effective_method<'a>(&'a self, dotted: &'a str) -> &'a str {
        self.remapped
            .get(dotted)
            .map_or(dotted, std::string::String::as_str)
    }

    /// One app's hooks.
    #[must_use]
    pub fn hooks(&self, app: &str) -> Option<&crate::hooks::Hooks> {
        self.hooks.get(app)
    }

    /// Every app's hooks, by app name.
    pub fn all_hooks(&self) -> impl Iterator<Item = (&String, &crate::hooks::Hooks)> {
        self.hooks.iter()
    }

    /// Every file the index read, for the cache's validity key
    /// ([31](../../../docs/specs/31-performance.md)).
    pub fn indexed_files(&self) -> impl Iterator<Item = PathBuf> + '_ {
        let doctypes = self
            .doctypes
            .values()
            .map(|doctype| doctype.path.to_path_buf());
        let fixtures = self.fixtures.keys().cloned();
        let modules = self.modules.keys().cloned();
        doctypes.chain(fixtures).chain(modules)
    }

    /// The files that decide *which* apps are indexed and in what order.
    ///
    /// A change to any of these changes the app set or the merge order, which
    /// no per-file patch can express: install an app and every later app's
    /// overrides shift behind it.
    pub fn discovery_inputs(&self) -> Vec<PathBuf> {
        let mut inputs = Vec::new();
        if let Some(bench) = self.workspace.detection.bench_root() {
            inputs.push(bench.join("sites/apps.txt"));
            inputs.push(bench.join("sites/apps.json"));
        }
        inputs.extend(
            self.workspace
                .apps
                .iter()
                .map(|app| app.package.join("modules.txt")),
        );
        inputs
    }

    /// Every file a fresh scan would read. Compared against the cached set so
    /// a file that appeared since has something to be missing from.
    pub fn discoverable_files(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.workspace.apps.iter().flat_map(|app| {
            app.artifacts
                .doctypes()
                .map(|record| record.path.clone())
                .chain(app.artifacts.fixtures.iter().cloned())
        })
    }

    /// Re-index one file, patching the graph in place.
    ///
    /// The merge is per-DocType-name, so rebuilding exactly the names a file
    /// touches yields the graph a full rebuild would
    /// ([32](../../../docs/specs/32-testing.md) asserts it). A file that no
    /// longer exists is removed from every layer it contributed to.
    pub fn reindex(&mut self, path: &Path) -> Reindexed {
        let mut affected: BTreeSet<String> = BTreeSet::new();
        let mut known = false;

        self.problems.retain(|problem| problem.path != path);

        let package = self
            .workspace
            .apps
            .iter()
            .find(|app| path.starts_with(&app.package))
            .map(|app| {
                (
                    app.package.clone(),
                    app.order,
                    Arc::<str>::from(app.name.as_str()),
                )
            });

        let Some((package, order, app)) = package else {
            return Reindexed {
                doctypes: Vec::new(),
                known: false,
            };
        };

        if path.extension().is_some_and(|extension| extension == "py") {
            known = true;
            self.reindex_python(path, &package, &mut affected);
        } else if fixtures::is_fixture(&package, path) {
            known = true;
            self.reindex_fixture(path, order, &app, &mut affected);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            known = true;
            self.reindex_doctype(path, &package, order, &app, &mut affected);
        }

        let mut raws = BTreeMap::new();
        for name in &affected {
            self.assemble(name, &mut raws);
        }
        self.sort_reports();

        Reindexed {
            doctypes: affected.into_iter().collect(),
            known,
        }
    }

    fn reindex_doctype(
        &mut self,
        path: &Path,
        package: &Path,
        order: usize,
        app: &Arc<str>,
        affected: &mut BTreeSet<String>,
    ) {
        for (name, sources) in &mut self.sources {
            if sources.iter().any(|source| source.path.as_ref() == path) {
                affected.insert(name.clone());
                sources.retain(|source| source.path.as_ref() != path);
            }
        }

        // A file that is gone contributes nothing at all — not even the
        // "unreadable" problem discovery would raise for one it can see but
        // cannot open. A rebuild would never have visited it.
        if !path.is_file() {
            return;
        }

        // The record rules of spec 03 still apply: a JSON file beside a DocType
        // is not automatically one, and a file they turn away must produce the
        // same problem discovery would have produced on a full build.
        match crate::artifacts::classify(path) {
            Ok(record) if record.is_doctype() => match read_and_parse(path) {
                Ok(raw) => {
                    affected.insert(raw.name.clone());
                    self.sources.entry(raw.name).or_default().push(Source {
                        order,
                        path: Arc::from(path.to_path_buf()),
                        app: Arc::clone(app),
                        package: Arc::from(package.to_path_buf()),
                    });
                }
                Err(message) => self.problems.push(Problem::new(path, message)),
            },
            Ok(_) => {}
            Err((_, problem)) => self.problems.extend(problem),
        }

        for sources in self.sources.values_mut() {
            sources.sort();
        }
    }

    fn reindex_fixture(
        &mut self,
        path: &Path,
        order: usize,
        app: &Arc<str>,
        affected: &mut BTreeSet<String>,
    ) {
        if let Some(previous) = self.fixtures.remove(path) {
            affected.extend(previous.fixture.doctypes().map(str::to_string));
        }

        let Ok(source) = std::fs::read_to_string(path) else {
            return;
        };
        match fixtures::read(&source) {
            Ok(fixture) => {
                affected.extend(fixture.doctypes().map(str::to_string));
                self.fixtures.insert(
                    path.to_path_buf(),
                    LayerTwo {
                        order,
                        app: Arc::clone(app),
                        fixture,
                    },
                );
            }
            Err(message) => self.problems.push(Problem::new(path, message)),
        }
    }

    fn reindex_python(&mut self, path: &Path, package: &Path, affected: &mut BTreeSet<String>) {
        self.modules.remove(path);
        if let Ok(source) = std::fs::read_to_string(path) {
            match python::read(&source) {
                Ok(module) => {
                    self.modules.insert(path.to_path_buf(), module);
                }
                Err(error) => self.problems.push(Problem::new(path, error.to_string())),
            }
        }

        self.whitelisted.retain(|_, method| method.path != path);
        if let Some(module) = self.modules.get(path)
            && let Some(dotted) = paths::dotted_module(package, path)
        {
            for method in whitelist::methods_of(module, path, &dotted) {
                self.whitelisted.insert(method.dotted.clone(), method);
            }
        }

        // A controller changing re-resolves its DocType's method set.
        let owner = self.doctypes.values().find(|doctype| {
            doctype
                .path
                .file_stem()
                .zip(path.file_stem())
                .is_some_and(|(json, python)| json == python)
                && doctype.path.parent() == path.parent()
        });
        if let Some(owner) = owner {
            affected.insert(owner.name.clone());
        }
    }

    /// A canonical, machine-independent dump of the whole graph.
    ///
    /// Interned symbols and hash iteration make the in-memory structure a bad
    /// equality target, and the property test of
    /// [32](../../../docs/specs/32-testing.md) needs "byte-identical" to mean
    /// something. Everything here is resolved to text, sorted, and relative to
    /// the workspace root. Build time is deliberately absent.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let root = self.workspace.detection.root.clone();
        let show = |path: &Path| -> String {
            path.strip_prefix(&root)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned()
        };

        let mut out = String::new();
        for app in &self.workspace.apps {
            out.push_str(&format!(
                "app {} order={} package={}\n",
                app.name,
                app.order,
                show(&app.package)
            ));
        }

        for doctype in self.doctypes.values() {
            out.push_str(&format!(
                "doctype {} app={} module={} child={} single={} submittable={} virtual={} tree={} autoname={:?} title_field={:?} sort_field={:?} search_fields={:?} path={} range={}\n",
                doctype.name,
                doctype.app,
                doctype.module.as_deref().unwrap_or(""),
                doctype.is_child,
                doctype.is_single,
                doctype.is_submittable,
                doctype.is_virtual,
                doctype.is_tree,
                doctype.autoname,
                doctype.title_field,
                doctype.sort_field,
                doctype.search_fields,
                show(&doctype.path),
                doctype.range,
            ));
            out.push_str("  order");
            for symbol in &doctype.field_order {
                out.push(' ');
                out.push_str(self.text(*symbol));
            }
            out.push('\n');

            for field in &doctype.fields {
                out.push_str(&format!(
                    "  field {} type={} layer={} app={} source={} range={} label={:?} options={:?} default={:?} reqd={} hidden={} read_only={} unique={} virtual={} permlevel={} depends_on={:?} fetch_from={:?}\n",
                    self.fieldname(field),
                    self.fieldtype(field),
                    field.provenance.layer.as_str(),
                    field.provenance.app,
                    field.provenance.source.as_deref().map_or_else(String::new, show),
                    field.range,
                    field.label,
                    field.options,
                    field.default,
                    field.reqd,
                    field.hidden,
                    field.read_only,
                    field.unique,
                    field.is_virtual,
                    field.permlevel,
                    field.depends_on,
                    field.fetch_from,
                ));
            }

            if let Some(controller) = &doctype.controller {
                out.push_str(&format!(
                    "  controller {} class={} range={}\n",
                    show(&controller.path),
                    controller.class_name,
                    controller.range
                ));
                for method in &controller.methods {
                    out.push_str(&format!(
                        "    method {} whitelisted={} range={}\n",
                        method.name, method.whitelisted, method.range
                    ));
                }
            }
        }

        for conflict in &self.conflicts {
            out.push_str(&format!(
                "conflict {} {} {:?} {}\n",
                conflict.code.as_str(),
                conflict.doctype,
                conflict.fieldname,
                conflict
                    .paths
                    .iter()
                    .map(|path| show(path))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }

        for method in self.whitelisted.values() {
            out.push_str(&format!(
                "whitelisted {} allow_guest={} methods={} params={} {} range={}\n",
                method.dotted,
                method.allow_guest,
                method.methods.join("|"),
                method.parameters.join("|"),
                show(&method.path),
                method.range,
            ));
        }

        for problem in &self.problems {
            out.push_str(&format!(
                "problem {} {}\n",
                show(&problem.path),
                problem.message
            ));
        }

        out
    }
}

/// The `frappe` version the vendored fieldtype tables were copied from.
///
/// A check that asserts invalidity from those tables fires only when the
/// workspace reports exactly this version
/// ([31](../../../docs/specs/31-performance.md)).
#[must_use]
pub fn vendored_snapshot() -> &'static str {
    fieldtypes::SNAPSHOT_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Index {
        Index::build(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .expect("repo root")
                .join("tests/fixtures/bench"),
        )
    }

    #[test]
    fn override_doctype_class_reaches_the_controller() {
        let index = fixture();
        let extras = index.controller_extras("Library Member");

        assert_eq!(extras.len(), 1, "{extras:?}");
        assert_eq!(extras[0].kind, ExtraKind::Override);
        assert_eq!(extras[0].class_name, "CustomLibraryMember");
        assert!(extras[0].path.ends_with("overrides.py"));
    }

    #[test]
    fn a_mixins_methods_join_the_method_set() {
        let index = fixture();
        let extras = index.controller_extras("Library Membership");

        assert_eq!(extras[0].kind, ExtraKind::Mixin);
        assert_eq!(extras[0].class_name, "MembershipMixin");
        // `recalculate` is declared by the mixin, not by the controller, and is
        // on the document at runtime either way.
        assert!(index.has_controller_method("Library Membership", "recalculate"));
        assert!(!index.has_controller_method("Library Membership", "no_such_method"));
    }

    #[test]
    fn a_remapped_whitelisted_path_reports_its_effective_target() {
        let index = fixture();
        assert_eq!(
            index.effective_method("library.api.get_member_summary"),
            "library.api.get_member_summary_v2"
        );
        assert_eq!(
            index.effective_method("library.api.ping"),
            "library.api.ping"
        );
    }

    #[test]
    fn an_unresolvable_override_contributes_nothing() {
        // circulation's `override_doctype_class` names a function where spec 12
        // requires a class. Nothing navigable comes of a wrong path kind.
        let index = fixture();
        assert!(index.controller_extras("Loan").is_empty());
    }

    #[test]
    fn hooks_are_read_for_every_installed_app() {
        let index = fixture();
        let apps: Vec<&String> = index.all_hooks().map(|(app, _)| app).collect();
        assert!(apps.contains(&&"library".to_string()), "{apps:?}");
        assert!(apps.contains(&&"circulation_app".to_string()), "{apps:?}");
        // The uninstalled app's hooks are not read at all.
        assert!(!apps.contains(&&"library_archive".to_string()), "{apps:?}");
    }
}
