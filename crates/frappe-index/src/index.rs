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
    /// Sites in the reverse occurrence table, or `None` until the scan that
    /// fills it has run ([21](../../../docs/specs/21-navigation.md)).
    pub occurrences: Option<usize>,
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
    /// Names it hangs off `self`, which join the DocType's attribute set.
    pub attributes: Vec<String>,
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

/// What the live site contributed — Layer 3 of
/// [04](../../../docs/specs/04-metadata-index.md).
///
/// One value rather than a map keyed by path, because it has no paths: it is
/// the whole of one site's customizations, and it is dropped whole.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct LayerThree {
    site: Arc<str>,
    fixture: Fixture,
    /// DocTypes created through the UI, from the site's meta. They exist in no
    /// file, and an installed app's DocType of the same name wins over them.
    doctypes: Vec<Raw>,
    /// Client and Server Scripts, by the file each was written to.
    scripts: Vec<crate::scripts::SiteScript>,
}

impl LayerThree {
    /// Every DocType this layer changes or defines.
    fn names(&self) -> BTreeSet<String> {
        self.fixture
            .doctypes()
            .map(std::string::ToString::to_string)
            .chain(self.doctypes.iter().map(|raw| raw.name.clone()))
            .collect()
    }
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
    site: Option<LayerThree>,
    modules: BTreeMap<PathBuf, python::Module>,
    hook_reads: BTreeSet<String>,
    handlers: BTreeSet<String>,
    occurrences: crate::occurrences::Occurrences,
    class_attributes: BTreeMap<String, BTreeSet<String>>,
    class_bases: BTreeMap<String, Vec<String>>,
    packages: Vec<(String, PathBuf)>,
    whitelisted: BTreeMap<String, WhitelistedMethod>,
    hooks: BTreeMap<String, crate::hooks::Hooks>,
    controller_extras: BTreeMap<String, Vec<ControllerExtra>>,
    remapped: BTreeMap<String, String>,
    conflicts: Vec<Conflict>,
    problems: Vec<Problem>,
    confidence: Confidence,
    build_time: Duration,
    /// Changes whenever the index does, and is never reused in a process, so
    /// work done under a read lock can check nothing moved before it writes.
    #[serde(skip, default = "next_revision")]
    revision: u64,
}

fn next_revision() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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
            site: None,
            modules: BTreeMap::new(),
            packages: Vec::new(),
            whitelisted: BTreeMap::new(),
            conflicts: Vec::new(),
            problems: Vec::new(),
            hooks: BTreeMap::new(),
            hook_reads: BTreeSet::new(),
            handlers: BTreeSet::new(),
            occurrences: crate::occurrences::Occurrences::default(),
            class_attributes: BTreeMap::new(),
            class_bases: BTreeMap::new(),
            controller_extras: BTreeMap::new(),
            remapped: BTreeMap::new(),
            confidence: Confidence::Partial,
            build_time: Duration::ZERO,
            revision: next_revision(),
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
            hook_reads: BTreeSet::new(),
            handlers: BTreeSet::new(),
            occurrences: crate::occurrences::Occurrences::default(),
            class_attributes: BTreeMap::new(),
            class_bases: BTreeMap::new(),
            controller_extras: BTreeMap::new(),
            remapped: BTreeMap::new(),
            workspace,
            interner: Interner::new(),
            doctypes: BTreeMap::new(),
            sources: BTreeMap::new(),
            fixtures: BTreeMap::new(),
            site: None,
            modules: BTreeMap::new(),
            packages,
            whitelisted: BTreeMap::new(),
            conflicts: Vec::new(),
            confidence: Confidence::Static,
            build_time: Duration::ZERO,
            revision: next_revision(),
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
        // …and before any JavaScript, because `doctype_js` decides which
        // DocType a form script anchors to, which spec 12 states as an
        // ordering requirement rather than a preference.
        index.read_javascript();
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

        self.collect_hook_reads();
        self.collect_classes();
    }

    /// Class name → what it hangs off `self`, and what it derives from.
    ///
    /// Keyed by bare class name rather than by dotted path: resolving a base
    /// would need this crate to follow imports, and the cost of a collision is
    /// one suppressed diagnostic, never a wrong one — which is the direction
    /// [23](../../../docs/specs/23-diagnostics.md) says to err in.
    fn collect_classes(&mut self) {
        self.class_attributes.clear();
        self.class_bases.clear();
        for module in self.modules.values() {
            for class in &module.classes {
                self.class_attributes
                    .entry(class.name.clone())
                    .or_default()
                    .extend(class.attributes.iter().cloned());
                let bases = self.class_bases.entry(class.name.clone()).or_default();
                for base in &class.bases {
                    // `frappe.model.document.Document` and `Document` are the
                    // same class to this map.
                    let bare = base.rsplit('.').next().unwrap_or(base);
                    if !bases.iter().any(|seen| seen == bare) {
                        bases.push(bare.to_string());
                    }
                }
            }
        }
    }

    /// Whether `class_name` derives, transitively, from any of `bases`.
    ///
    /// Uses the same bare-name class graph as [`Index::has_attribute`], which
    /// exists whether or not anything asks this question. Matching by bare
    /// name can only ever say yes too often, and the cost of that here is a
    /// code lens on a class that is not a test — visible, harmless, and
    /// nothing like the cost of no lens at all on the 484 erpnext test classes
    /// that derive from `ERPNextTestSuite`
    /// ([25](../../../docs/specs/25-test-runner.md)).
    #[must_use]
    pub fn derives_from(&self, class_name: &str, bases: &[&str]) -> bool {
        let mut pending = vec![class_name.to_string()];
        let mut seen: BTreeSet<String> = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            if bases.contains(&current.as_str()) {
                return true;
            }
            if let Some(inherited) = self.class_bases.get(&current) {
                pending.extend(inherited.iter().cloned());
            }
        }
        false
    }

    /// Whether `class_name` or anything it derives from assigns `name`.
    fn class_assigns(&self, class_name: &str, name: &str) -> bool {
        let mut pending = vec![class_name.to_string()];
        let mut seen: BTreeSet<String> = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            if self
                .class_attributes
                .get(&current)
                .is_some_and(|attributes| attributes.contains(name))
            {
                return true;
            }
            if let Some(bases) = self.class_bases.get(&current) {
                pending.extend(bases.iter().cloned());
            }
        }
        false
    }

    /// Every form-handler name the workspace's JavaScript calls by name.
    ///
    /// Only the names are kept. The trees are not: this exists to answer one
    /// yes-or-no question per key, and holding 1 350 parsed files to answer it
    /// would cost more memory than the whole metadata graph.
    fn read_javascript(&mut self) {
        let files: Vec<PathBuf> = self
            .workspace
            .apps
            .iter()
            .flat_map(|app| {
                Walk::new(&app.package)
                    .filter(|path| path.extension().is_some_and(|extension| extension == "js"))
            })
            .collect();

        self.handlers = files
            .into_par_iter()
            .filter_map(|path| {
                let source = std::fs::read_to_string(&path).ok()?;
                // The query cannot match text that is not there, and most of a
                // bench's JavaScript is not form script. Parsing only the
                // files that could match takes this pass from 770 ms to a
                // fraction of it.
                if !source.contains("trigger(") && !source.contains("events.") {
                    return None;
                }
                frappe_syntax::handlers::read(&source).ok()
            })
            .flatten()
            .collect();
    }

    /// Every hook key any app asks for, across the workspace.
    ///
    /// Recomputed rather than maintained: a key that stayed in the set after
    /// its last reader was deleted would silence a real `unknown-hook`, and
    /// the whole set is a few thousand short strings.
    fn collect_hook_reads(&mut self) {
        self.hook_reads = self
            .modules
            .values()
            .flat_map(|module| module.hook_reads.iter().cloned())
            .collect();
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
            self.assemble_from_site(name);
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

    /// A DocType created through the UI, which only the live site defines.
    fn assemble_from_site(&mut self, name: &str) {
        let Some(live) = &self.site else {
            return;
        };
        let Some(raw) = live.doctypes.iter().find(|raw| raw.name == name).cloned() else {
            return;
        };
        let app = Arc::clone(&live.site);
        let provenance = Provenance {
            layer: Layer::Site,
            app: Arc::clone(&app),
            // No file: what eviction keys on, and what tells navigation there
            // is nowhere to go.
            source: None,
        };

        let mut doctype = DocType {
            name: raw.name,
            module: raw.module,
            app,
            path: Arc::from(Path::new("")),
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
            // Positions in the meta the site sent, which no editor has open.
            range: raw.range,
            meta_ranges: raw.meta_ranges,
            provenance,
        };
        self.merge_layer_two(&mut doctype);
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

        // Layer 3 last, so the live site wins: a Custom Field added through
        // Customize Form is the current truth, and a fixtures file holding an
        // older copy of it is a snapshot
        // ([04](../../../docs/specs/04-metadata-index.md)).
        if let Some(live) = &self.site {
            let app = Arc::clone(&live.site);
            for record in &live.fixture.custom_fields {
                if record.doctype != doctype.name {
                    continue;
                }
                let fieldname = self.interner.intern(&record.field.fieldname);
                if doctype.field(fieldname).is_some() {
                    // Already present from a fixtures file that exported the
                    // same customization. Not a collision — the same field.
                    continue;
                }
                let field = record.field.clone().into_field(
                    &mut self.interner,
                    Provenance {
                        layer: Layer::Site,
                        app: Arc::clone(&app),
                        // No file. This is what eviction keys on.
                        source: None,
                    },
                );
                let after = record
                    .insert_after
                    .as_deref()
                    .and_then(|name| self.interner.get(name));
                insert_after(&mut doctype.fields, field, after);
                insert_order(&mut doctype.field_order, fieldname, after);
            }

            for record in &live.fixture.property_setters {
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
                attributes: self
                    .modules
                    .get(&candidate.path)
                    .and_then(|module| module.class(&candidate.name))
                    .map(|class| class.attributes.clone())
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

    /// Whether the controller hangs `name` off `self`.
    ///
    /// A controller is an ordinary Python class: `self._attachments`,
    /// `self.flags`, a cached total. None of those is a field and every one is
    /// read back later, so without this the controller's own state is reported
    /// as missing fields — 675 of them across `frappe` and `erpnext`
    /// ([32](../../../docs/specs/32-testing.md)).
    #[must_use]
    pub fn has_attribute(&self, doctype: &str, name: &str) -> bool {
        let own = self
            .doctype(doctype)
            .and_then(|doctype| doctype.controller.as_ref())
            .is_some_and(|controller| controller.attributes.iter().any(|a| a == name));

        if own {
            return true;
        }

        let extra = self.controller_extras.get(doctype).is_some_and(|extras| {
            extras
                .iter()
                .any(|e| e.attributes.iter().any(|a| a == name))
        });
        if extra {
            return true;
        }

        // erpnext's controllers derive from `AccountsController` and
        // `StockController`, which set most of the working state the leaf
        // classes read back.
        self.doctype(doctype)
            .and_then(|doctype| doctype.controller.as_ref())
            .is_some_and(|controller| self.class_assigns(&controller.class_name, name))
    }

    /// A DocType that holds rows of `child` and declares `fieldname`.
    ///
    /// `child.member_name` inside a `Library Loan Item` binding is the classic
    /// confusion: the row does not carry the parent's fields, and Frappe
    /// returns `None` rather than failing
    /// ([10](../../../docs/specs/10-surface-python.md)).
    #[must_use]
    pub fn parent_with_field(&self, child: &str, fieldname: &str) -> Option<&str> {
        self.parents(child)
            .find(|parent| parent.has_field(&self.interner, fieldname))
            .map(|parent| parent.name.as_str())
    }

    /// Every DocType with a `Table` field holding rows of `child`, once each.
    /// Empty when `child` is not a child DocType.
    pub fn parents<'s, 'c>(
        &'s self,
        child: &'c str,
    ) -> impl Iterator<Item = &'s DocType> + use<'s, 'c> {
        let is_child = self.doctype(child).is_some_and(|doctype| doctype.is_child);
        self.doctypes.values().filter(move |parent| {
            is_child
                && parent.fields.iter().any(|field| {
                    self.interner.resolve(field.fieldtype) == "Table"
                        && field
                            .options
                            .as_deref()
                            .is_some_and(|options| options.eq_ignore_ascii_case(child))
                })
        })
    }

    /// Whether `fieldname` is a `Table` field, on any DocType, holding rows of
    /// `child`.
    ///
    /// Grid row events are registered against the **child**: `grid.js` fires
    /// `script_manager.trigger(fieldname + "_add", row.doctype, row.name)`, so
    /// `items_add` written under `frappe.ui.form.on("Sales Order Item", …)` is
    /// the documented shape even though `items` is a field of the parent
    /// ([11](../../../docs/specs/11-surface-javascript.md)).
    #[must_use]
    pub fn is_grid_field(&self, fieldname: &str, child: &str) -> bool {
        self.doctypes.values().any(|doctype| {
            doctype.fields.iter().any(|field| {
                self.interner.resolve(field.fieldname) == fieldname
                    && self.interner.resolve(field.fieldtype) == "Table"
                    && field
                        .options
                        .as_deref()
                        .is_some_and(|options| options.eq_ignore_ascii_case(child))
            })
        })
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

    /// The dotted module path of a file, and the app it belongs to.
    ///
    /// The reverse of [07](../../../docs/specs/07-dotted-paths.md): a path
    /// under `<package>/a/b.py` is `<package>.a.b`, and `__init__.py` is the
    /// package it sits in rather than a module of its own.
    #[must_use]
    pub fn dotted_module(&self, path: &Path) -> Option<(&str, String)> {
        let app = self
            .workspace
            .apps
            .iter()
            .find(|app| path.starts_with(&app.package))?;
        let relative = path.strip_prefix(app.package.parent()?).ok()?;

        let mut parts: Vec<String> = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect();
        let last = parts.pop()?;
        let stem = last.strip_suffix(".py")?;
        if stem != "__init__" {
            parts.push(stem.to_string());
        }
        Some((app.name.as_str(), parts.join(".")))
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
            occurrences: self.occurrences.scanned.then(|| self.occurrences.len()),
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

    /// Take a live site's customizations as Layer 3.
    ///
    /// The records arrive in the shape a fixtures file holds them, so they go
    /// through Layer 2's reader rather than a second one that would eventually
    /// disagree with it about what a `depends_on` is
    /// ([05](../../../docs/specs/05-live-bridge.md)).
    ///
    /// `doctypes` holds the meta of each DocType created through the UI, which
    /// is the same shape as a DocType JSON and goes through the same reader.
    /// One that does not parse is left out, never guessed at.
    ///
    /// Returns the DocTypes the site touched.
    pub fn apply_site(
        &mut self,
        site: &str,
        custom_fields: &str,
        property_setters: &str,
        doctypes: &[String],
    ) -> Vec<String> {
        self.revision = next_revision();
        let mut fixture = crate::fixtures::read(custom_fields).unwrap_or_default();
        let setters = crate::fixtures::read(property_setters).unwrap_or_default();
        fixture.property_setters.extend(setters.property_setters);
        fixture.custom_fields.extend(setters.custom_fields);
        let live = LayerThree {
            site: Arc::from(site),
            fixture,
            doctypes: doctypes
                .iter()
                .filter_map(|meta| doctype::parse(meta).ok())
                .collect(),
            scripts: Vec::new(),
        };

        // Whatever the previous sync touched has to be rebuilt too, or a field
        // deleted through Customize Form stays in the graph forever.
        let previous = self
            .site
            .as_ref()
            .map(LayerThree::names)
            .unwrap_or_default();
        let touched = live.names();

        self.site = Some(live);
        self.confidence = Confidence::Full;

        let affected: Vec<String> = touched.union(&previous).cloned().collect();
        self.reassemble(&affected);
        affected
    }

    /// Drop Layer 3 without a rebuild.
    ///
    /// `reachable` says which of spec 05's two off states this is: a bridge
    /// that was never configured leaves the user's own picture intact
    /// (`Static`), and one that was configured and failed means the server
    /// knows its picture is incomplete (`Degraded`), which suppresses the
    /// absence checks of [23](../../../docs/specs/23-diagnostics.md).
    pub fn evict_site(&mut self, reachable: bool) -> Vec<String> {
        self.revision = next_revision();
        let affected: Vec<String> = self
            .site
            .take()
            .map(|live| live.names().into_iter().collect())
            .unwrap_or_default();

        self.confidence = if reachable {
            Confidence::Static
        } else {
            Confidence::Degraded
        };
        self.reassemble(&affected);
        affected
    }

    /// Record where a sync wrote the site's scripts. Without a site layer
    /// there is nothing for them to belong to, and they are dropped.
    pub fn set_site_scripts(&mut self, scripts: Vec<crate::scripts::SiteScript>) {
        if let Some(live) = &mut self.site {
            live.scripts = scripts;
            self.revision = next_revision();
        }
    }

    /// Every script the live site holds.
    #[must_use]
    pub fn site_scripts(&self) -> &[crate::scripts::SiteScript] {
        self.site
            .as_ref()
            .map_or(&[], |live| live.scripts.as_slice())
    }

    /// The scripts that run for one DocType.
    pub fn site_scripts_for<'a>(
        &'a self,
        doctype: &'a str,
    ) -> impl Iterator<Item = &'a crate::scripts::SiteScript> {
        self.site_scripts()
            .iter()
            .filter(move |script| script.doctype.as_deref() == Some(doctype))
    }

    /// The site Layer 3 came from, for hover and the status report.
    #[must_use]
    pub fn site(&self) -> Option<&str> {
        self.site.as_ref().map(|live| live.site.as_ref())
    }

    /// Rebuild exactly these DocTypes from every layer.
    fn reassemble(&mut self, names: &[String]) {
        if names.is_empty() {
            return;
        }
        let mut raws = BTreeMap::new();
        for name in names {
            self.assemble(name, &mut raws);
        }
        self.sort_reports();
    }

    /// The reverse occurrence table — [21](../../../docs/specs/21-navigation.md).
    #[must_use]
    pub fn occurrences(&self) -> &crate::occurrences::Occurrences {
        &self.occurrences
    }

    /// Fill the reverse occurrence table.
    ///
    /// Kept out of `build`: a full extract-and-resolve pass over every source
    /// file costs seconds, against a two-second budget for the whole cold
    /// index ([31](../../../docs/specs/31-performance.md)). The server runs it
    /// after the index is published, and the cache carries the result, so a
    /// warm start pays nothing.
    pub fn scan_occurrences(&mut self) {
        let table = self.collect_occurrences();
        self.install_occurrences(table);
    }

    /// The whole reverse occurrence table, computed without changing the
    /// index, so a server can build it under a read lock while requests are
    /// still being answered. [`Self::revision`] tells it whether the table is
    /// still current when it comes to install it.
    #[must_use]
    pub fn collect_occurrences(&self) -> crate::occurrences::Occurrences {
        let files: Vec<PathBuf> = self
            .workspace
            .apps
            .iter()
            .flat_map(|app| {
                Walk::new(&app.package).filter(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "py" || extension == "js")
                })
            })
            .collect();

        let found: Vec<(PathBuf, Vec<(crate::occurrences::Key, Range)>)> = files
            .into_par_iter()
            .filter_map(|path| {
                let source = std::fs::read_to_string(&path).ok()?;
                Some((self.occurrences_in(&path, &source), path))
            })
            .map(|(found, path)| (path, found))
            .collect();

        let mut table = crate::occurrences::Occurrences::default();
        for (path, keys) in found {
            let path: Arc<Path> = Arc::from(path.as_path());
            for (key, range) in keys {
                table.insert(key, &path, range);
            }
        }

        self.schema_occurrences(&mut table);
        self.hook_occurrences(&mut table);
        self.fixture_occurrences(&mut table);
        table.sort();
        table.scanned = true;
        table
    }

    /// Take a table from [`Self::collect_occurrences`].
    pub fn install_occurrences(&mut self, table: crate::occurrences::Occurrences) {
        self.occurrences = table;
        self.revision = next_revision();
    }

    /// Changes on every mutation and is never reused in a process.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Re-scan one file's occurrences, and those of every DocType it touched.
    ///
    /// A DocType JSON contributes its own rows; a source file contributes
    /// whatever it names. Both are keyed by file, so the fix is to forget the
    /// file and read it again — the table stays exactly what a full scan would
    /// have produced ([32](../../../docs/specs/32-testing.md) asserts it).
    fn reindex_occurrences(&mut self, path: &Path, affected: &BTreeSet<String>) {
        if !self.occurrences.scanned {
            return;
        }

        self.occurrences.forget(path);
        if let Ok(source) = std::fs::read_to_string(path) {
            let found = self.occurrences_in(path, &source);
            let owned: Arc<Path> = Arc::from(path);
            for (key, range) in found {
                self.occurrences.insert(key, &owned, range);
            }
        }

        // A DocType JSON's own rows are derived from the merged graph rather
        // than from the file, so they are rebuilt for every name it touched.
        for name in affected {
            if let Some(doctype) = self.doctypes.get(name) {
                self.occurrences.forget(&doctype.path);
            }
        }
        let mut table = std::mem::take(&mut self.occurrences);
        self.schema_occurrences(&mut table);
        table.sort();
        self.occurrences = table;
    }

    /// DocType names used as hook map keys.
    ///
    /// `hooks.py` is a `.py` file, but a `doc_events` key is a dict literal
    /// and the general extractor has no reason to call it a DocType. Only the
    /// schemas written `Map<DocTypeName, …>` in
    /// [12](../../../docs/specs/12-surface-hooks.md) are walked, which is the
    /// same rule the diagnostics use.
    fn hook_occurrences(&self, table: &mut crate::occurrences::Occurrences) {
        use crate::occurrences::Key;
        use frappe_syntax::generated::Schema;
        use frappe_syntax::hooks::Value;

        let files: Vec<PathBuf> = self
            .workspace
            .apps
            .iter()
            .map(|app| app.package.join("hooks.py"))
            .collect();

        let mut found: Vec<(String, PathBuf, Range)> = Vec::new();
        for path in files {
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(assignments) = frappe_syntax::hooks::read(&source) else {
                continue;
            };
            for assignment in assignments {
                let keyed_by_doctype = frappe_syntax::generated::hook_schema(&assignment.key)
                    .is_some_and(|schema| {
                        matches!(
                            schema,
                            Schema::DocEvents
                                | Schema::MapToClass
                                | Schema::MapToClasses
                                | Schema::MapToCallable
                                | Schema::MapToFile
                        )
                    });
                if !keyed_by_doctype {
                    continue;
                }
                let Value::Map(entries) = &assignment.value else {
                    continue;
                };
                for (key, _) in entries {
                    if self.doctypes.contains_key(&key.text) {
                        found.push((key.text.clone(), path.clone(), Range::from(key.span)));
                    }
                }
            }
        }

        for (name, path, range) in found {
            let path: Arc<Path> = Arc::from(path.as_path());
            table.insert(Key::DocType(name), &path, range);
        }
    }

    /// DocType names a Layer-2 fixtures file targets — spec 21 counts fixtures
    /// as a surface, and a Custom Field is how half a bench's schema arrives.
    fn fixture_occurrences(&self, table: &mut crate::occurrences::Occurrences) {
        use crate::occurrences::Key;

        let mut found: Vec<(Key, PathBuf, Range)> = Vec::new();
        for (path, layer) in &self.fixtures {
            for record in &layer.fixture.custom_fields {
                found.push((
                    Key::DocType(record.doctype.clone()),
                    path.clone(),
                    record.field.range,
                ));
                found.push((
                    Key::Field(record.doctype.clone(), record.field.fieldname.clone()),
                    path.clone(),
                    record.field.range,
                ));
            }
            for record in &layer.fixture.property_setters {
                found.push((
                    Key::DocType(record.doctype.clone()),
                    path.clone(),
                    record.range,
                ));
            }
        }

        for (key, path, range) in found {
            let path: Arc<Path> = Arc::from(path.as_path());
            table.insert(key, &path, range);
        }
    }

    /// Occurrences in one source file, without touching the table.
    fn occurrences_in(&self, path: &Path, source: &str) -> Vec<(crate::occurrences::Key, Range)> {
        use crate::occurrences::Key;

        let Some(language) = frappe_syntax::Language::from_path(path) else {
            return Vec::new();
        };
        let Ok(mut parser) = language.parser() else {
            return Vec::new();
        };
        let Some(tree) = parser.parse(source, None) else {
            return Vec::new();
        };

        let controller = self
            .doctypes
            .values()
            .find(|doctype| doctype.controller.as_ref().is_some_and(|c| c.path == path));
        let context = frappe_syntax::Context {
            frm_doctype: self.doctype_bound_to(path),
            self_doctype: controller.map(|doctype| doctype.name.as_str()),
            self_class: controller
                .and_then(|doctype| doctype.controller.as_ref())
                .map(|controller| controller.class_name.as_str()),
        };

        let matches = frappe_syntax::extract(language, &tree, source, context);
        crate::resolve::resolve_all(self, &matches)
            .into_iter()
            .filter_map(|resolved| {
                let key = match resolved.target {
                    // Only what resolved. An `Unknown` anchor would flood the
                    // result set, and spec 21 excludes it by name.
                    crate::resolve::Target::DocType { name } => Key::DocType(name),
                    crate::resolve::Target::Field {
                        doctype, fieldname, ..
                    } => Key::Field(doctype, fieldname),
                    _ => return None,
                };
                Some((key, Range::from(resolved.span)))
            })
            .collect()
    }

    /// The occurrences the schema itself holds: a `Link` naming a DocType, a
    /// `fetch_from` naming a field, the field declarations themselves.
    fn schema_occurrences(&self, table: &mut crate::occurrences::Occurrences) {
        use crate::occurrences::Key;

        let mut found: Vec<(Key, PathBuf, Range)> = Vec::new();
        for doctype in self.doctypes.values() {
            let path: PathBuf = doctype.path.to_path_buf();
            for field in &doctype.fields {
                let Some(range) = field.ranges.options else {
                    continue;
                };
                let fieldtype = self.interner.resolve(field.fieldtype);
                if matches!(fieldtype, "Link" | "Table" | "Table MultiSelect")
                    && let Some(options) = field.options.as_deref()
                    && self.doctypes.contains_key(options)
                {
                    found.push((Key::DocType(options.to_string()), path.clone(), range));
                }
            }
            // The declaration is a reference to itself: `references` on a field
            // that does not include where it is declared sends the reader
            // nowhere ([21](../../../docs/specs/21-navigation.md)).
            for field in &doctype.fields {
                found.push((
                    Key::Field(
                        doctype.name.clone(),
                        self.interner.resolve(field.fieldname).to_string(),
                    ),
                    path.clone(),
                    // The `fieldname` value, not the whole field object: a
                    // reference list of 40-line ranges is unreadable.
                    field.ranges.fieldname.unwrap_or(field.range),
                ));
            }
        }

        for (key, path, range) in found {
            let path: Arc<Path> = Arc::from(path.as_path());
            table.insert(key, &path, range);
        }
    }

    /// Whether anything in the workspace calls a form handler by this name —
    /// `frm.trigger("x")` or `frm.events.x(frm)`
    /// ([11](../../../docs/specs/11-surface-javascript.md)).
    #[must_use]
    pub fn calls_handler(&self, name: &str) -> bool {
        self.handlers.contains(name)
    }

    /// Whether any app in the workspace reads `key` back through `get_hooks`.
    ///
    /// The vendored registry covers the hooks *frappe* reads. An app may
    /// invent its own — erpnext defines and consumes `invoice_doctypes` — and
    /// those are as real as the built-in ones
    /// ([12](../../../docs/specs/12-surface-hooks.md)).
    #[must_use]
    pub fn reads_hook(&self, key: &str) -> bool {
        self.hook_reads.contains(key)
    }

    /// Form scripts bound to a DocType by `doctype_js` and its family.
    ///
    /// A script need not sit beside the DocType JSON: an app can bind one from
    /// anywhere, including to a DocType another app defines, and the hook is
    /// the only record of it ([12](../../../docs/specs/12-surface-hooks.md)).
    #[must_use]
    pub fn scripts_for(&self, doctype: &str) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for app in &self.workspace.apps {
            let Some(hooks) = self.hooks.get(&app.name) else {
                continue;
            };
            for written in hooks.scripts_for(doctype) {
                if let Some(path) = crate::hooks::asset_path(&app.package, &app.name, written)
                    && !found.contains(&path)
                {
                    found.push(path);
                }
            }
        }
        found
    }

    /// The DocType a form script is bound to, if `doctype_js` bound it.
    ///
    /// The reverse of [`Index::scripts_for`], and what lets `frm` anchor in a
    /// file that has no `frappe.ui.form.on` of its own
    /// ([11](../../../docs/specs/11-surface-javascript.md)).
    #[must_use]
    pub fn doctype_bound_to(&self, path: &Path) -> Option<&str> {
        self.doctypes
            .keys()
            .find(|name| self.scripts_for(name).iter().any(|bound| bound == path))
            .map(String::as_str)
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
                // Python is walked rather than taken from the artifact lists:
                // discovery records records and assets, and a controller is
                // neither. A new `.py` that never appears here is a
                // whitelisted method the cache will never learn about.
                .chain(
                    Walk::new(&app.package)
                        .filter(|path| path.extension().is_some_and(|e| e == "py")),
                )
        })
    }

    /// Replace the discovered picture with a freshly walked one.
    ///
    /// The cache stores the workspace it was built from, so the artifact lists
    /// in it describe the bench as it was. Files added since exist in neither
    /// the lists nor the stamps, which is how a cache serves an index of a
    /// bench that no longer exists ([31](../../../docs/specs/31-performance.md)).
    pub(crate) fn refresh_workspace(&mut self, workspace: Workspace) {
        self.problems.retain(|problem| {
            !workspace
                .problems
                .iter()
                .any(|old| old.path == problem.path)
        });
        self.problems.extend(workspace.problems.clone());
        self.workspace = workspace;
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

        let problems = self.problems.len();
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

        // A path in no app changes nothing, and moving the revision for it would
        // throw away an occurrence scan in flight for no reason.
        let Some((package, order, app)) = package else {
            if self.problems.len() != problems {
                self.revision = next_revision();
            }
            return Reindexed {
                doctypes: Vec::new(),
                known: false,
            };
        };
        self.revision = next_revision();

        if path.extension().is_some_and(|extension| extension == "py") {
            known = true;
            self.reindex_python(path, &package, &mut affected);
            self.collect_hook_reads();
            self.collect_classes();
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
        self.reindex_occurrences(path, &affected);

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
