//! Protocol handlers — spec 02.
//!
//! Every capability advertised in [`crate::capabilities`] answers here, even
//! before it can answer usefully: an unimplemented method returns
//! `method_not_found`, which the client logs as an error for the rest of the
//! session. The feature tasks replace the empty bodies; the empty bodies are
//! what keeps the log clean until then.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde_json::{Value, json};
use tower_lsp_server::jsonrpc::{Error, ErrorCode, Result};
use tower_lsp_server::ls_types::request::{
    GotoImplementationParams, GotoImplementationResponse, GotoTypeDefinitionParams,
    GotoTypeDefinitionResponse,
};
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer};

use frappe_index::index::{Confidence, Index, IndexStatus};

use crate::analysis::{self, Found};
use crate::buffers::Buffers;
use crate::capabilities::{ClientSupport, server_capabilities};
use crate::config::{Config, ConfigError, ConfigSources};
use crate::watcher::{self, FileWatcher};
use crate::{completion, definition, diagnostics, hover, locations, symbols, testing};
use std::time::Duration;

pub const SERVER_NAME: &str = "frappe-lsp";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
pub struct Backend {
    client: Client,
    state: Arc<RwLock<State>>,
    /// Read by every request, replaced once by the background build. Requests
    /// arriving before it finishes are served against the partial index and
    /// say so, rather than blocking or lying
    /// ([02](../../../docs/specs/02-architecture.md)).
    index: Arc<RwLock<Index>>,
}

#[derive(Debug, Default)]
struct State {
    support: ClientSupport,
    sources: ConfigSources,
    config: Config,
    buffers: Buffers,
    /// Where the index was built from, for a forced rebuild.
    root: Option<PathBuf>,
    /// The fallback watcher. Held so it lives as long as the session; dropping
    /// it releases the platform watches.
    watcher: Option<FileWatcher>,
    /// Reported to the user once the client is listening; anything sent during
    /// `initialize` is dropped by most clients.
    pending: Vec<String>,
    /// Files a conflict was published to that the user never opened, so the
    /// next build can clear the ones that no longer conflict.
    conflicting: Vec<Uri>,
    /// The site the user picked this session, when the bench has several.
    /// Not written back to any file: choosing which database a test suite
    /// runs against is the user's to make permanent, in their own config
    /// ([25](../../../docs/specs/25-test-runner.md)).
    chosen_site: Option<String>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(RwLock::new(State::default())),
            index: Arc::new(RwLock::new(Index::partial())),
        }
    }

    /// Run `f` against a `hooks.py` position.
    ///
    /// A separate path from [`Self::with_position`] because `hooks.py` is not
    /// a call surface: the meaning of a string there comes from the hook it
    /// sits under, not from a call shape
    /// ([12](../../../docs/specs/12-surface-hooks.md)).
    fn with_hook<T>(
        &self,
        uri: &Uri,
        position: Position,
        f: impl FnOnce(&Index, &crate::hooks::Located, Range) -> Option<T>,
    ) -> Option<T> {
        let path = uri.to_file_path().map(|path| path.into_owned())?;
        if !crate::hooks::is_hooks_file(&path) {
            return None;
        }

        let index = self.index.read().expect("index lock poisoned by a panic");
        let state = self.state.read().expect("state lock poisoned by a panic");
        let buffer = state.buffers.get(uri)?;

        let source = buffer.text();
        let offset = buffer.offset(frappe_syntax::Position::new(
            position.line,
            position.character,
        ));
        let located = crate::hooks::at(&index, &source, offset, &path)?;

        let lines = locations::LineIndex::new(&source, state.support.position_encoding);
        let range = Range {
            start: lines.position(located.span.start),
            end: lines.position(located.span.end),
        };
        f(&index, &located, range)
    }

    /// Run `f` against whatever sits at `position`.
    ///
    /// The three features differ only in what they do with the answer, so the
    /// locate-match-resolve half of the request flow lives here once
    /// ([02](../../../docs/specs/02-architecture.md)). `None` from anywhere in
    /// the chain — an unopened buffer, an unsupported language, a position that
    /// is not a Frappe surface — means the feature has nothing to say, which is
    /// the common case and not an error.
    fn with_position<T>(
        &self,
        uri: &Uri,
        position: Position,
        f: impl FnOnce(&PositionContext<'_>) -> Option<T>,
    ) -> Option<T> {
        let index = self.index.read().expect("index lock poisoned by a panic");
        let state = self.state.read().expect("state lock poisoned by a panic");

        let buffer = state.buffers.get(uri)?;
        if !analysis::supported(buffer.language()) {
            return None;
        }

        let offset = buffer.offset(frappe_syntax::Position::new(
            position.line,
            position.character,
        ));
        let path = uri.to_file_path().map(|path| path.into_owned());
        let context = analysis::context_for(&index, path.as_deref());
        let found = analysis::at(&index, buffer, offset, context)?;

        f(&PositionContext {
            index: &index,
            buffer,
            found,
            support: state.support,
        })
    }

    /// Keep the index current with the filesystem.
    ///
    /// The client's watcher is preferred where it can be registered: the editor
    /// already knows what changed, and a second recursive watch over a bench
    /// costs thousands of inotify handles for the same answer (spec 30). The
    /// `notify` fallback exists for clients that cannot.
    async fn start_watching(&self, root: &Path) {
        if self.write(|state| state.support.dynamic_watcher_registration) {
            let watchers = ["**/*.py", "**/*.json", "**/patches.txt"]
                .into_iter()
                .map(|glob| FileSystemWatcher {
                    glob_pattern: GlobPattern::String(glob.to_string()),
                    kind: None,
                })
                .collect();
            let registration = Registration {
                id: "frappe-lsp/watchedFiles".to_string(),
                method: "workspace/didChangeWatchedFiles".to_string(),
                register_options: serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                    watchers,
                })
                .ok(),
            };

            match self.client.register_capability(vec![registration]).await {
                Ok(()) => {
                    tracing::info!("watching through the client");
                    return;
                }
                // Advertising the capability and refusing the registration is
                // rare enough to be worth a line, and falling back is correct.
                Err(error) => tracing::warn!(%error, "client refused the watcher registration"),
            }
        }

        // Establishing the watches walks every directory under each package,
        // which on a real bench takes about a second — long enough to be worth
        // keeping off the thread that answers requests.
        let index = Arc::clone(&self.index);
        let fallback = root.to_path_buf();
        let started = tokio::task::spawn_blocking(move || {
            let packages: Vec<PathBuf> = index
                .read()
                .expect("index lock poisoned by a panic")
                .apps()
                .iter()
                .map(|app| app.package.clone())
                .collect();
            // Before the first build finishes there are no packages yet, and
            // the bench root is the only directory known to exist.
            let mut roots = if packages.is_empty() {
                vec![fallback.clone()]
            } else {
                packages
            };
            // `sites/` itself, so an app installed while the editor is open is
            // noticed. Not recursively: site directories churn constantly and
            // hold nothing the index reads.
            let sites = fallback.join("sites");
            if sites.is_dir() {
                roots.push(sites);
            }
            watcher::start(&roots, Some(fallback), Arc::clone(&index))
        })
        .await;

        match started {
            Ok(Ok(watcher)) => {
                self.write(|state| state.watcher = Some(watcher));
                tracing::info!("watching the workspace");
            }
            // A workspace with no watcher still answers; it just goes stale.
            Ok(Err(error)) => tracing::warn!(%error, "could not watch the workspace"),
            Err(error) => tracing::warn!(%error, "watcher setup panicked"),
        }
    }

    /// Compute and publish diagnostics for one buffer.
    ///
    /// Push, because pull is advertised only to clients that asked for it and
    /// the fallback has to work either way
    /// ([30](../../../docs/specs/30-configuration.md)).
    async fn publish(&self, uri: Uri) {
        let Some(path) = uri.to_file_path().map(|path| path.into_owned()) else {
            return;
        };

        let computed = {
            let index = self.index.read().expect("index lock poisoned by a panic");
            let state = self.state.read().expect("state lock poisoned by a panic");
            let Some(buffer) = state.buffers.get(&uri) else {
                return;
            };
            diagnostics::for_buffer(&index, &state.config, &path, &buffer.text())
        };

        self.client.publish_diagnostics(uri, computed, None).await;
    }

    /// Publish after a quiet period.
    ///
    /// Spec 31 puts diagnostics at most every 150 ms per buffer: recomputing
    /// per keystroke spends the whole budget on text the user has already
    /// replaced. A newer edit supersedes an older one by version.
    fn publish_debounced(&self, uri: &Uri, version: i32) {
        let uri = uri.clone();
        let client = self.client.clone();
        let index = Arc::clone(&self.index);
        let state = Arc::clone(&self.state);

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;

            let Some(path) = uri.to_file_path().map(|path| path.into_owned()) else {
                return;
            };
            let computed = {
                let index = index.read().expect("index lock poisoned by a panic");
                let state = state.read().expect("state lock poisoned by a panic");
                let Some(buffer) = state.buffers.get(&uri) else {
                    return;
                };
                // Superseded: a later edit publishes for itself, and this one
                // would overwrite it with an older answer.
                if buffer.version() != version {
                    return;
                }
                diagnostics::for_buffer(&index, &state.config, &path, &buffer.text())
            };
            client.publish_diagnostics(uri, computed, None).await;
        });
    }

    /// Publish conflicts against files nobody opened.
    ///
    /// `duplicate-doctype` and `custom-field-collision` land on two files by
    /// definition, and the second one is usually in an app the user is not
    /// editing. Publishing only to open buffers means the diagnostic that says
    /// *which other app* redefines a DocType is the one nobody ever sees
    /// ([23](../../../docs/specs/23-diagnostics.md)).
    ///
    /// Anything published here is cleared on the next build, so a conflict the
    /// user resolves does not sit in their problem list forever.
    /// The status of the index as it stands — spec 30's `indexStatus`.
    fn status(&self) -> IndexStatus {
        self.index
            .read()
            .expect("index lock poisoned by a panic")
            .status()
    }

    /// Build the index off the request path.
    ///
    /// Discovery and the parse are CPU-bound and take hundreds of milliseconds
    /// on a real bench, which is several times the budget for a single
    /// request; `initialize` must not wait for either
    /// ([31](../../../docs/specs/31-performance.md)).
    fn build_in_background(&self, root: PathBuf) {
        let index = Arc::clone(&self.index);
        let state = Arc::clone(&self.state);
        let client = self.client.clone();
        let cache = self.write(|state| {
            state
                .config
                .index
                .cache
                .then(|| state.config.index.cache_dir.clone())
        });

        tokio::spawn(async move {
            let building = {
                let index = Arc::clone(&index);
                let root = root.clone();
                let cache = cache.clone();
                tokio::task::spawn_blocking(move || build(&index, &root, cache.as_deref()))
            };
            if building.await.is_err() {
                tracing::error!("the index build panicked");
                return;
            }
            // The conflict pair is published only now, because until the build
            // lands there is no conflict to publish.
            publish_conflicts(&client, &index, &state).await;

            // The reverse occurrence table costs about as much as the build
            // itself, so it runs after the index is already answering
            // ([21](../../../docs/specs/21-navigation.md)). A warm start finds
            // it in the cache and skips this.
            let scanning = {
                let index = Arc::clone(&index);
                tokio::task::spawn_blocking(move || {
                    let scanned = index
                        .read()
                        .expect("index lock poisoned by a panic")
                        .occurrences()
                        .scanned;
                    if scanned {
                        return;
                    }
                    let started = std::time::Instant::now();
                    let mut guard = index.write().expect("index lock poisoned by a panic");
                    guard.scan_occurrences();
                    tracing::info!(
                        sites = guard.occurrences().len(),
                        elapsed = ?started.elapsed(),
                        "reverse occurrence table built"
                    );
                })
            };
            if scanning.await.is_err() {
                tracing::error!("the occurrence scan panicked");
                return;
            }

            // Written after the scan so the cache carries the table; a warm
            // start then answers `references` immediately.
            if let Some(directory) = cache.as_deref() {
                let guard = index.read().expect("index lock poisoned by a panic");
                let _ = frappe_index::cache::save(&guard, &root, directory);
            }
        });
    }

    /// Guards are never held across an `.await`: the trait requires `Send`
    /// futures, so this is checked at compile time rather than by review.
    fn write<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.state.write().expect("state lock poisoned by a panic"))
    }
}

/// Publish conflicts against files nobody opened.
///
/// `duplicate-doctype` and `custom-field-collision` land on two files by
/// definition, and the second one is usually in an app the user is not
/// editing. Publishing only to open buffers means the diagnostic that says
/// *which other app* redefines a DocType is the one nobody ever sees
/// ([23](../../../docs/specs/23-diagnostics.md)).
///
/// Anything published here is cleared on the next build, so a conflict the user
/// resolves does not sit in their problem list forever.
async fn publish_conflicts(
    client: &Client,
    index: &Arc<RwLock<Index>>,
    state: &Arc<RwLock<State>>,
) {
    let (config, open) = {
        let guard = state.read().expect("state lock poisoned by a panic");
        (guard.config.clone(), guard.buffers.uris())
    };

    // Everything is computed before the first await: the index guard is not
    // `Send`, and holding it across one would not compile.
    let computed: Vec<(Uri, Vec<Diagnostic>)> = {
        let index = index.read().expect("index lock poisoned by a panic");
        let mut seen: Vec<Uri> = Vec::new();
        let mut computed = Vec::new();
        for path in index
            .conflicts()
            .iter()
            .flat_map(|conflict| conflict.paths.iter())
        {
            let Some(uri) = crate::locations::uri_of(path) else {
                continue;
            };
            // An open buffer is published from its own text, on its own
            // schedule; publishing here as well would race it.
            if open.contains(&uri) || seen.contains(&uri) {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(path) else {
                continue;
            };
            seen.push(uri.clone());
            computed.push((uri, diagnostics::for_buffer(&index, &config, path, &source)));
        }
        computed
    };

    let published: Vec<Uri> = computed.iter().map(|(uri, _)| uri.clone()).collect();
    let stale: Vec<Uri> = {
        let mut guard = state.write().expect("state lock poisoned by a panic");
        let stale = guard
            .conflicting
            .iter()
            .filter(|uri| !published.contains(uri))
            .cloned()
            .collect();
        guard.conflicting = published;
        stale
    };

    for (uri, diagnostics) in computed {
        client.publish_diagnostics(uri, diagnostics, None).await;
    }
    for uri in stale {
        client.publish_diagnostics(uri, Vec::new(), None).await;
    }
}

/// Build or warm the index in place, then write the cache.
fn build(index: &Arc<RwLock<Index>>, root: &Path, cache: Option<&Path>) {
    // A cache is an optimisation: anything wrong with it means a full
    // scan, never an error (spec 31).
    let warm = cache.and_then(|directory| frappe_index::cache::load(root, directory));
    let from_cache = warm.is_some();
    let built = warm.unwrap_or_else(|| Index::build(root));

    let status = built.status();
    *index.write().expect("index lock poisoned by a panic") = built;
    tracing::info!(
        apps = status.apps,
        doctypes = status.doctypes,
        fields = status.fields,
        whitelisted = status.whitelisted_methods,
        conflicts = status.conflicts,
        problems = status.problems,
        frappe = status.frappe_version.as_deref().unwrap_or("unknown"),
        elapsed = ?status.build_time,
        cached = from_cache,
        "index built"
    );

    // Written only after the index is published. Encoding a few MiB
    // and stamping every file it read takes longer than the build
    // itself, and doing it first would hold the answer back for a
    // start the cache exists to make faster.
    if let Some(directory) = cache
        && !from_cache
    {
        let guard = index.read().expect("index lock poisoned by a panic");
        match frappe_index::cache::save(&guard, root, directory) {
            Ok(()) => tracing::debug!("index cache written"),
            // Costs a slower start next time and nothing else.
            Err(error) => tracing::debug!(%error, "could not write the index cache"),
        }
    }
}

impl Backend {
    fn report(&self, errors: Vec<ConfigError>) {
        self.write(|state| {
            for error in errors {
                tracing::error!(%error, "configuration");
                state.pending.push(error.to_string());
            }
        });
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let support = ClientSupport::detect(&params.capabilities);
        let roots = workspace_roots(&params);
        let (sources, errors) = ConfigSources::load(&roots, params.initialization_options);

        let config = sources.resolve();
        tracing::info!(
            client = params.client_info.as_ref().map_or("unknown", |i| &i.name),
            encoding = ?support.position_encoding,
            bench_root = ?sources.bench_root(),
            "initializing"
        );
        tracing::debug!(?config, "effective configuration");

        let root = sources.bench_root().map(Path::to_path_buf);
        self.write(|state| {
            state.support = support;
            state.sources = sources;
            state.config = config;
            state.root.clone_from(&root);
        });
        self.report(errors);

        Ok(InitializeResult {
            capabilities: server_capabilities(support),
            server_info: Some(ServerInfo {
                name: SERVER_NAME.to_string(),
                version: Some(VERSION.to_string()),
            }),
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        let pending = self.write(|state| std::mem::take(&mut state.pending));
        for message in pending {
            self.client.log_message(MessageType::ERROR, message).await;
        }

        // Started here rather than in `initialize` so the client is already
        // listening: the build logs what it found, and a client that has not
        // sent `initialized` discards it.
        match self.write(|state| state.root.clone()) {
            Some(root) => {
                tracing::info!(root = %root.display(), "indexing");
                self.build_in_background(root.clone());
                self.start_watching(&root).await;
            }
            // No bench: the session stays alive and Frappe-specific answers
            // stay empty (specs/02).
            None => tracing::info!("no bench detected; serving nothing Frappe-specific"),
        }

        tracing::info!("ready");
    }

    async fn shutdown(&self) -> Result<()> {
        tracing::info!("shutting down");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        let uri = document.uri.clone();

        let opened = self.write(|state| {
            let encoding = state.support.position_encoding;
            state.buffers.open(
                document.uri,
                &document.language_id,
                &document.text,
                document.version,
                encoding,
            )
        });

        match opened {
            Ok(true) => {
                tracing::debug!(uri = uri.as_str(), "opened");
                self.publish(uri.clone()).await;
            }
            Ok(false) => tracing::trace!(
                uri = uri.as_str(),
                language = document.language_id,
                "ignored: no grammar"
            ),
            // A grammar that will not load is a build fault, and it costs this
            // buffer every feature. Say so once, where the user can see it.
            Err(error) => {
                tracing::error!(uri = uri.as_str(), %error, "could not parse buffer");
                self.client
                    .log_message(MessageType::ERROR, error.to_string())
                    .await;
            }
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        let known =
            self.write(|state| state.buffers.change(&uri, &params.content_changes, version));

        // Editing a buffer the server was never told about means sync state has
        // diverged; every position from here on would be computed against the
        // wrong text. A buffer with no grammar was never opened either, and
        // that is the common case, so this stays below warning.
        if known {
            self.publish_debounced(&uri, version);
        } else {
            tracing::debug!(uri = uri.as_str(), "change for an unopened buffer");
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        tracing::debug!(uri = uri.as_str(), "saved");

        // Spec 02: a save on an indexed file re-parses that file only and
        // patches the graph. The watcher that catches edits made outside the
        // editor is 009; this is the half the protocol already tells us about.
        let Some(path) = uri.to_file_path().map(|path| path.into_owned()) else {
            return;
        };
        let index = Arc::clone(&self.index);
        tokio::task::spawn_blocking(move || {
            let mut index = index.write().expect("index lock poisoned by a panic");
            let reindexed = index.reindex(&path);
            if reindexed.known {
                tracing::debug!(
                    path = %path.display(),
                    doctypes = ?reindexed.doctypes,
                    "re-indexed"
                );
            }
        });
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.write(|state| state.buffers.close(&uri));
        // A closed buffer keeps its diagnostics in the editor's list forever
        // unless they are explicitly cleared.
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        let result = self.write(|state| {
            state
                .sources
                .set_workspace_configuration(params.settings)
                .map(|()| {
                    state.config = state.sources.resolve();
                    state.config.clone()
                })
        });

        match result {
            Ok(config) => tracing::debug!(?config, "configuration changed"),
            Err(error) => {
                tracing::error!(%error, "configuration");
                self.client
                    .log_message(MessageType::ERROR, error.to_string())
                    .await;
            }
        }
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        tracing::debug!(
            added = params.event.added.len(),
            removed = params.event.removed.len(),
            "workspace folders changed"
        );
    }

    /// The client's own watcher, which is the preferred source when it has one
    /// ([30](../../../docs/specs/30-configuration.md)).
    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let paths: Vec<PathBuf> = params
            .changes
            .iter()
            .filter_map(|change| change.uri.to_file_path().map(|path| path.into_owned()))
            .filter(|path| watcher::interesting(path))
            .collect();
        if paths.is_empty() {
            return;
        }

        tracing::debug!(changes = paths.len(), "watched files changed");
        let index = Arc::clone(&self.index);
        let state = Arc::clone(&self.state);
        let client = self.client.clone();
        tokio::spawn(async move {
            let reindexing = {
                let index = Arc::clone(&index);
                tokio::task::spawn_blocking(move || {
                    let mut index = index.write().expect("index lock poisoned by a panic");
                    for path in paths {
                        index.reindex(&path);
                    }
                })
            };
            if reindexing.await.is_err() {
                tracing::error!("re-indexing panicked");
                return;
            }
            // A DocType added or removed changes who conflicts with whom, and
            // the other side of the pair is usually not open.
            publish_conflicts(&client, &index, &state).await;
        });
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let position = params.text_document_position;
        let max_items = self.write(|state| state.config.completion.max_items);
        let support = self.write(|state| state.support);

        // `hooks.py` is not resolved through the shape tables, so it takes its
        // own path ([12](../../../docs/specs/12-surface-hooks.md)).
        if let Some(response) = self.with_hook(
            &position.text_document.uri,
            position.position,
            |index, located, range| {
                completion::complete_hook(index, located, Some(range), support, max_items)
            },
        ) {
            return Ok(Some(response));
        }

        Ok(
            self.with_position(&position.text_document.uri, position.position, |ctx| {
                completion::complete(ctx.index, ctx.buffer, &ctx.found, ctx.support, max_items)
            }),
        )
    }

    async fn completion_resolve(&self, item: CompletionItem) -> Result<CompletionItem> {
        let markdown = self.write(|state| state.support.markdown);
        let index = self.index.read().expect("index lock poisoned by a panic");
        Ok(completion::resolve(&index, item, markdown))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let position = params.text_document_position_params;
        let uri = &position.text_document.uri;
        let markdown_client = self.write(|state| state.support.markdown);

        if let Some(found) = self.with_hook(uri, position.position, |index, located, range| {
            let body = hover::hook_markdown(index, &located.target)?;
            Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: if markdown_client {
                        MarkupKind::Markdown
                    } else {
                        MarkupKind::PlainText
                    },
                    value: if markdown_client {
                        body
                    } else {
                        hover::as_plain_text(&body)
                    },
                }),
                range: Some(range),
            })
        }) {
            return Ok(Some(found));
        }

        Ok(self.with_position(uri, position.position, |ctx| {
            let body = hover::markdown(ctx.index, &ctx.found)?;
            let range = ctx.origin_range();

            let contents = if ctx.support.markdown {
                HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: body,
                })
            } else {
                HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::PlainText,
                    value: hover::as_plain_text(&body),
                })
            };
            Some(Hover {
                contents,
                range: Some(range),
            })
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let position = params.text_document_position_params;
        let uri = &position.text_document.uri;

        // Read before the call, never inside it: `with_hook` holds a read
        // guard on the state lock for the whole closure, and asking for the
        // write guard in there deadlocks the thread against itself.
        let support = self.write(|state| state.support);

        // `hooks.py` is never a call surface, so it is asked first and the
        // shape tables are not consulted for it at all (spec 12).
        if let Some(response) = self.with_hook(uri, position.position, |index, located, range| {
            locations::links(
                &definition::hook_targets(index, &located.target),
                Some(range),
                support.position_encoding,
                support.location_link,
            )
            .map(Into::into)
        }) {
            return Ok(Some(response));
        }

        Ok(self.with_position(uri, position.position, |ctx| {
            let targets = definition::targets(ctx.index, &ctx.found);
            locations::links(
                &targets,
                Some(ctx.origin_range()),
                ctx.support.position_encoding,
                ctx.support.location_link,
            )
            .map(Into::into)
        }))
    }

    /// The DocType JSON of whatever the position is anchored to — spec 21.
    async fn goto_type_definition(
        &self,
        params: GotoTypeDefinitionParams,
    ) -> Result<Option<GotoTypeDefinitionResponse>> {
        let position = params.text_document_position_params;
        Ok(
            self.with_position(&position.text_document.uri, position.position, |ctx| {
                let name = ctx.found.target.doctype()?;
                let doctype = ctx.index.doctype(name)?;
                locations::links(
                    &[locations::Target::whole(
                        doctype.path.to_path_buf(),
                        doctype.range,
                    )],
                    Some(ctx.origin_range()),
                    ctx.support.position_encoding,
                    ctx.support.location_link,
                )
                .map(Into::into)
            }),
        )
    }

    /// The classes `override_doctype_class` and `extend_doctype_class`
    /// contribute to a DocType's controller — spec 21.
    async fn goto_implementation(
        &self,
        params: GotoImplementationParams,
    ) -> Result<Option<GotoImplementationResponse>> {
        let position = params.text_document_position_params;
        Ok(
            self.with_position(&position.text_document.uri, position.position, |ctx| {
                let doctype = ctx.found.target.doctype()?;
                locations::links(
                    &definition::implementations(ctx.index, doctype),
                    Some(ctx.origin_range()),
                    ctx.support.position_encoding,
                    ctx.support.location_link,
                )
                .map(Into::into)
            }),
        )
    }

    /// `textDocument/references` — spec 21's workspace-wide reverse lookup.
    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let position = params.text_document_position;
        let include = params.context.include_declaration;
        Ok(
            self.with_position(&position.text_document.uri, position.position, |ctx| {
                symbols::references(ctx.index, &ctx.found, include)
            }),
        )
    }

    /// `textDocument/documentSymbol` — the outline of a DocType JSON,
    /// `hooks.py` or `patches.txt`, and nothing for a controller.
    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let Some(path) = uri.to_file_path().map(|path| path.into_owned()) else {
            return Ok(None);
        };

        // The buffer's text when it is open, the file's otherwise: a JSON the
        // user is editing has ranges the file on disk no longer has.
        let source = {
            let state = self.state.read().expect("state lock poisoned by a panic");
            state
                .buffers
                .get(&uri)
                .map(|buffer| buffer.text().to_string())
        };
        let Some(source) = source.or_else(|| std::fs::read_to_string(&path).ok()) else {
            return Ok(None);
        };

        let index = self.index.read().expect("index lock poisoned by a panic");
        let found = symbols::document_symbols(&index, &path, &source);
        Ok((!found.is_empty()).then_some(DocumentSymbolResponse::Nested(found)))
    }

    /// `workspace/symbol` — spec 21's fuzzy search over DocTypes, whitelisted
    /// methods, reports, pages and web forms.
    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<WorkspaceSymbolResponse>> {
        let index = self.index.read().expect("index lock poisoned by a panic");
        Ok(Some(WorkspaceSymbolResponse::Nested(
            symbols::workspace_symbols(&index, &params.query),
        )))
    }

    /// `textDocument/codeLens` — one lens per runnable test scope, spec 25.
    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = params.text_document.uri;
        let Some(path) = uri.to_file_path().map(|path| path.into_owned()) else {
            return Ok(None);
        };

        let source = {
            let state = self.state.read().expect("state lock poisoned by a panic");
            state
                .buffers
                .get(&uri)
                .map(|buffer| buffer.text().to_string())
        };
        let Some(source) = source.or_else(|| std::fs::read_to_string(&path).ok()) else {
            return Ok(None);
        };

        let config = self.write(|state| state.config.clone());
        let chosen = self.write(|state| state.chosen_site.clone());
        let index = self.index.read().expect("index lock poisoned by a panic");
        let lenses = testing::lenses(&index, &config, chosen.as_deref(), &path, &source);
        Ok((!lenses.is_empty()).then_some(lenses))
    }

    /// `workspace/executeCommand` — the server-run fallback of spec 25.
    ///
    /// Strictly worse than the client running it: no TTY, no interactivity,
    /// and the output arrives as log lines. Opt-in for exactly that reason.
    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
        if params.command == crate::capabilities::SELECT_SITE {
            return self.select_site(&params).await;
        }
        if params.command != crate::capabilities::RUN_TEST {
            return Ok(None);
        }

        // The argv is rebuilt here rather than taken from the request. What a
        // lens carries has passed through the client and is not evidence of
        // anything; the scope descriptor is re-validated against the index and
        // the program comes from configuration this process trusts
        // ([25](../../../docs/specs/25-test-runner.md)).
        let Some(scope) = params
            .arguments
            .first()
            .and_then(|value| serde_json::from_value::<testing::Invocation>(value.clone()).ok())
        else {
            return Ok(None);
        };
        let config = self.write(|state| state.config.clone());
        let chosen = self.write(|state| state.chosen_site.clone());
        let argv = {
            let index = self.index.read().expect("index lock poisoned by a panic");
            testing::rebuild(&index, &config, chosen.as_deref(), &scope)
        };
        let Some(argv) = argv else {
            self.client
                .log_message(
                    MessageType::ERROR,
                    "that test scope no longer resolves in this workspace",
                )
                .await;
            return Ok(None);
        };
        let Some((program, rest)) = argv.split_first() else {
            return Ok(None);
        };
        let root = self.write(|state| state.root.clone());

        // The client asked what to run and runs it itself: it gets a TTY, its
        // own scrollback, and the command this server built rather than one it
        // had to assemble from a scope it cannot validate
        // ([25](../../../docs/specs/25-test-runner.md)).
        let runner = self.write(|state| state.config.test.runner.clone());
        if runner != "server" {
            return Ok(Some(json!({
                "argv": argv,
                "cwd": root.as_ref().map(|root| root.display().to_string()),
            })));
        }

        tracing::info!(argv = ?argv, "running tests");
        self.client
            .log_message(MessageType::INFO, format!("$ {}", argv.join(" ")))
            .await;

        let mut command = tokio::process::Command::new(program);
        command.args(rest);
        if let Some(root) = root {
            command.current_dir(root);
        }
        // Output is logged, never parsed: a failing test is not a defect in
        // the line it points at, and turning it into a diagnostic would make
        // spec 23's precision rule meaningless.
        match command.output().await {
            Ok(output) => {
                for stream in [&output.stdout, &output.stderr] {
                    for line in String::from_utf8_lossy(stream).lines() {
                        self.client
                            .log_message(MessageType::LOG, line.to_string())
                            .await;
                    }
                }
                Ok(Some(json!({"exitCode": output.status.code()})))
            }
            Err(error) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("could not run {program}: {error}"),
                    )
                    .await;
                Ok(None)
            }
        }
    }

    /// Quick fixes for the diagnostics the client is showing — spec 23.
    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let index = self.index.read().expect("index lock poisoned by a panic");
        let state = self.state.read().expect("state lock poisoned by a panic");
        let Some(buffer) = state.buffers.get(&uri) else {
            return Ok(None);
        };

        let actions = diagnostics::fixes(&index, &uri, &buffer.text(), &params.context.diagnostics);
        Ok((!actions.is_empty()).then_some(actions))
    }

    /// The pull transport, for clients that advertise it.
    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        let items = params
            .text_document
            .uri
            .to_file_path()
            .map(|path| path.into_owned())
            .map(|path| {
                let index = self.index.read().expect("index lock poisoned by a panic");
                let state = self.state.read().expect("state lock poisoned by a panic");
                state
                    .buffers
                    .get(&params.text_document.uri)
                    .map(|buffer| {
                        diagnostics::for_buffer(&index, &state.config, &path, &buffer.text())
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        Ok(DocumentDiagnosticReportResult::Report(
            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: None,
                    items,
                },
            }),
        ))
    }
}

impl Backend {
    /// `frappe-lsp.selectSite` — ask which site to run against.
    ///
    /// Spec 25 refuses to guess when a bench has several sites, because
    /// guessing runs a suite against the wrong database. Asking is the other
    /// half of that: the user says which one, the choice holds for the
    /// session, and the lenses become runnable.
    async fn select_site(&self, params: &ExecuteCommandParams) -> Result<Option<Value>> {
        let sites: Vec<String> = params
            .arguments
            .first()
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default();
        if sites.is_empty() {
            return Ok(None);
        }

        let actions = sites
            .iter()
            .map(|site| MessageActionItem {
                title: site.clone(),
                properties: std::collections::HashMap::new(),
            })
            .collect();
        let picked = self
            .client
            .show_message_request(
                MessageType::INFO,
                "Which site should tests run against?",
                Some(actions),
            )
            .await?;

        let Some(picked) = picked else {
            // Dismissed. Nothing is chosen, and nothing is assumed.
            return Ok(None);
        };
        // Only a site the bench actually has: the answer comes back through
        // the client, and the list is the server's.
        if !sites.contains(&picked.title) {
            return Ok(None);
        }

        tracing::info!(site = %picked.title, "site chosen for this session");
        self.write(|state| state.chosen_site = Some(picked.title.clone()));
        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "tests will run against {}; set test.site to keep it across sessions",
                    picked.title
                ),
            )
            .await;
        // The lenses were titled "choose a site" and carried no command that
        // runs anything; they are a different set now.
        //
        // Detached on purpose: the choice is already stored, and the refresh
        // is a request *to* the client. Awaiting it would let a client that
        // ignores `codeLens/refresh` hang the action the user just took.
        let client = self.client.clone();
        tokio::spawn(async move {
            let _ = client.code_lens_refresh().await;
        });
        Ok(Some(json!({"site": picked.title})))
    }

    /// `frappe-lsp/indexStatus` — app and DocType counts, confidence, build
    /// time. Spec 32's integration tests assert against this rather than
    /// reaching inside the process, and a bug report starts here.
    pub async fn index_status(&self, _: Option<Value>) -> Result<Value> {
        let status = self.status();
        Ok(json!({
            "confidence": confidence_name(status.confidence),
            "apps": status.apps,
            "doctypes": status.doctypes,
            "fields": status.fields,
            "controllers": status.controllers,
            "whitelistedMethods": status.whitelisted_methods,
            "modules": status.modules,
            "conflicts": status.conflicts,
            "problems": status.problems,
            // Absent until the scan has run, so a client can tell "no
            // references" from "not yet" (specs/21).
            "occurrences": status.occurrences,
            "frappeVersion": status.frappe_version,
            "buildTimeMs": status.build_time.as_millis() as u64,
        }))
    }

    /// `frappe-lsp/reindex` — force a full rebuild.
    ///
    /// Returns as soon as the rebuild is scheduled. Waiting for it would block
    /// the request for as long as the build takes, and the client can poll
    /// `indexStatus` for the answer.
    pub async fn reindex(&self, _: Option<Value>) -> Result<Value> {
        let Some(root) = self.write(|state| state.root.clone()) else {
            return Err(Error {
                code: ErrorCode::InvalidRequest,
                message: "no bench detected; there is nothing to index".into(),
                data: None,
            });
        };

        *self.index.write().expect("index lock poisoned by a panic") = Index::partial();
        self.build_in_background(root.clone());
        Ok(json!({ "started": true, "root": root.display().to_string() }))
    }

    /// `frappe-lsp/refreshBridge` — re-sync Layer 3.
    ///
    /// The bridge does not exist yet. An explicit error beats
    /// `method_not_found`: one says the feature is off, the other says the
    /// server is broken.
    pub async fn refresh_bridge(&self, _: Option<Value>) -> Result<Value> {
        Err(Error {
            code: ErrorCode::InvalidRequest,
            message: "the live bridge is not enabled in this build".into(),
            data: None,
        })
    }
}

/// What a request needs about the position it was asked about.
struct PositionContext<'a> {
    index: &'a Index,
    buffer: &'a frappe_syntax::Buffer,
    found: Found,
    support: ClientSupport,
}

impl PositionContext<'_> {
    /// The string interior, as the protocol wants it — spec 21 requires the
    /// editor to highlight the name and not the quotes.
    fn origin_range(&self) -> Range {
        let span = self.found.origin(self.buffer);
        let text = self.buffer.text();
        let lines = locations::LineIndex::new(&text, self.support.position_encoding);
        Range {
            start: lines.position(span.start),
            end: lines.position(span.end),
        }
    }
}

fn confidence_name(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::Full => "full",
        Confidence::Static => "static",
        Confidence::Degraded => "degraded",
        Confidence::Partial => "partial",
    }
}

#[expect(deprecated, reason = "root_uri is the only root a 3.15 client sends")]
fn workspace_roots(params: &InitializeParams) -> Vec<PathBuf> {
    if let Some(folders) = &params.workspace_folders {
        let roots: Vec<PathBuf> = folders
            .iter()
            .filter_map(|folder| folder.uri.to_file_path().map(|path| path.into_owned()))
            .collect();
        if !roots.is_empty() {
            return roots;
        }
    }

    params
        .root_uri
        .as_ref()
        .and_then(|uri| uri.to_file_path().map(|path| path.into_owned()))
        .into_iter()
        .collect()
}
