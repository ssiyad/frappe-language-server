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

use crate::buffers::Buffers;
use crate::capabilities::{ClientSupport, server_capabilities};
use crate::config::{Config, ConfigError, ConfigSources};

pub const SERVER_NAME: &str = "frappe-lsp";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
pub struct Backend {
    client: Client,
    state: RwLock<State>,
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
    /// Reported to the user once the client is listening; anything sent during
    /// `initialize` is dropped by most clients.
    pending: Vec<String>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: RwLock::new(State::default()),
            index: Arc::new(RwLock::new(Index::partial())),
        }
    }

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
        tokio::task::spawn_blocking(move || {
            let built = Index::build(&root);
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
                "index built"
            );
        });
    }

    /// Guards are never held across an `.await`: the trait requires `Send`
    /// futures, so this is checked at compile time rather than by review.
    fn write<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.state.write().expect("state lock poisoned by a panic"))
    }

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
                self.build_in_background(root);
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
            Ok(true) => tracing::debug!(uri = uri.as_str(), "opened"),
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
        if !known {
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
        self.write(|state| state.buffers.close(&params.text_document.uri));
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

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        tracing::debug!(changes = params.changes.len(), "watched files changed");
    }

    async fn completion(&self, _: CompletionParams) -> Result<Option<CompletionResponse>> {
        Ok(None)
    }

    async fn completion_resolve(&self, item: CompletionItem) -> Result<CompletionItem> {
        Ok(item)
    }

    async fn hover(&self, _: HoverParams) -> Result<Option<Hover>> {
        Ok(None)
    }

    async fn goto_definition(
        &self,
        _: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        Ok(None)
    }

    async fn goto_type_definition(
        &self,
        _: GotoTypeDefinitionParams,
    ) -> Result<Option<GotoTypeDefinitionResponse>> {
        Ok(None)
    }

    async fn goto_implementation(
        &self,
        _: GotoImplementationParams,
    ) -> Result<Option<GotoImplementationResponse>> {
        Ok(None)
    }

    async fn references(&self, _: ReferenceParams) -> Result<Option<Vec<Location>>> {
        Ok(None)
    }

    async fn document_symbol(
        &self,
        _: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        Ok(None)
    }

    async fn symbol(&self, _: WorkspaceSymbolParams) -> Result<Option<WorkspaceSymbolResponse>> {
        Ok(None)
    }

    async fn code_action(&self, _: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        Ok(None)
    }

    async fn diagnostic(
        &self,
        _: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        Ok(DocumentDiagnosticReportResult::Report(
            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: None,
                    items: Vec::new(),
                },
            }),
        ))
    }
}

impl Backend {
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
