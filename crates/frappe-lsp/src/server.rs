//! Protocol handlers — spec 02.
//!
//! Every capability advertised in [`crate::capabilities`] answers here, even
//! before it can answer usefully: an unimplemented method returns
//! `method_not_found`, which the client logs as an error for the rest of the
//! session. The feature tasks replace the empty bodies; the empty bodies are
//! what keeps the log clean until then.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::request::{
    GotoImplementationParams, GotoImplementationResponse, GotoTypeDefinitionParams,
    GotoTypeDefinitionResponse,
};
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer};

use crate::capabilities::{ClientSupport, server_capabilities};
use crate::config::{Config, ConfigError, ConfigSources};
use crate::document::Document;

pub const SERVER_NAME: &str = "frappe-lsp";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
pub struct Backend {
    client: Client,
    state: RwLock<State>,
}

#[derive(Debug, Default)]
struct State {
    support: ClientSupport,
    sources: ConfigSources,
    config: Config,
    documents: HashMap<Uri, Document>,
    /// Reported to the user once the client is listening; anything sent during
    /// `initialize` is dropped by most clients.
    pending: Vec<String>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: RwLock::new(State::default()),
        }
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

        self.write(|state| {
            state.support = support;
            state.sources = sources;
            state.config = config;
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
        tracing::info!("ready");
    }

    async fn shutdown(&self) -> Result<()> {
        tracing::info!("shutting down");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        tracing::debug!(uri = document.uri.as_str(), "opened");
        self.write(|state| {
            state.documents.insert(
                document.uri,
                Document::new(&document.text, document.version),
            );
        });
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        let known = self.write(|state| {
            let encoding = state.support.position_encoding;
            match state.documents.get_mut(&uri) {
                Some(document) => {
                    document.apply(params.content_changes, version, encoding);
                    true
                }
                None => false,
            }
        });

        // Editing a buffer the server was never told about means sync state has
        // diverged; every position from here on would be computed against the
        // wrong text.
        if !known {
            tracing::warn!(uri = uri.as_str(), "change for an unopened document");
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        tracing::debug!(uri = params.text_document.uri.as_str(), "saved");
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.write(|state| state.documents.remove(&params.text_document.uri));
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
