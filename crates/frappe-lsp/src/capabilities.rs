//! Capability negotiation — spec 30.
//!
//! What the client supports is read once at `initialize` and kept; spec 30
//! requires client capabilities to be honoured, not assumed, and every entry in
//! [`ClientSupport`] has a fallback the rest of the server has to respect.

use tower_lsp_server::ls_types::{
    ClientCapabilities, CodeActionProviderCapability, CodeLensOptions, CompletionOptions,
    CompletionOptionsCompletionItem, DiagnosticOptions, DiagnosticServerCapabilities,
    ExecuteCommandOptions, HoverProviderCapability, ImplementationProviderCapability, MarkupKind,
    OneOf, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TypeDefinitionProviderCapability,
    WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities,
};

use frappe_syntax::PositionEncoding;

use crate::buffers;

/// Trigger characters of spec 30: the quote that opens a DocType or fieldname
/// literal, the dot of a dotted path, and the bracket of a `filters` list.
const COMPLETION_TRIGGERS: [&str; 4] = ["\"", "'", ".", "["];

/// Identifier reported on pull diagnostics; matches the `frappe-lsp/` code
/// prefix of spec 23.
const DIAGNOSTIC_SOURCE: &str = "frappe-lsp";

/// The command a lens carries, and the one `workspace/executeCommand` accepts
/// for the server-run fallback of [25](../../../docs/specs/25-test-runner.md).
pub const RUN_TEST: &str = "frappe-lsp.runTest";

/// Asks which site to run against, when a bench has several and none is
/// configured. Answering is what makes the lenses runnable.
pub const SELECT_SITE: &str = "frappe-lsp.selectSite";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClientSupport {
    pub position_encoding: PositionEncoding,
    pub location_link: bool,
    pub markdown: bool,
    pub label_details: bool,
    pub pull_diagnostics: bool,
    pub dynamic_watcher_registration: bool,
}

impl ClientSupport {
    pub fn detect(capabilities: &ClientCapabilities) -> Self {
        let text_document = capabilities.text_document.as_ref();

        let position_encoding = buffers::negotiate(
            capabilities
                .general
                .as_ref()
                .and_then(|general| general.position_encodings.as_deref()),
        );

        let location_link = text_document
            .and_then(|caps| caps.definition.as_ref())
            .and_then(|caps| caps.link_support)
            .unwrap_or(false);

        let markdown = text_document
            .and_then(|caps| caps.hover.as_ref())
            .and_then(|caps| caps.content_format.as_ref())
            .is_some_and(|formats| formats.contains(&MarkupKind::Markdown));

        let label_details = text_document
            .and_then(|caps| caps.completion.as_ref())
            .and_then(|caps| caps.completion_item.as_ref())
            .and_then(|item| item.label_details_support)
            .unwrap_or(false);

        let pull_diagnostics = text_document.is_some_and(|caps| caps.diagnostic.is_some());

        let dynamic_watcher_registration = capabilities
            .workspace
            .as_ref()
            .and_then(|caps| caps.did_change_watched_files.as_ref())
            .and_then(|caps| caps.dynamic_registration)
            .unwrap_or(false);

        Self {
            position_encoding,
            location_link,
            markdown,
            label_details,
            pull_diagnostics,
            dynamic_watcher_registration,
        }
    }
}

/// Advertise exactly the list in spec 30. Formatting, rename, signature help,
/// semantic tokens and inlay hints stay unadvertised: other servers own them.
pub fn server_capabilities(support: ClientSupport) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(buffers::encoding_kind(support.position_encoding)),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                ..Default::default()
            },
        )),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(true),
            trigger_characters: Some(COMPLETION_TRIGGERS.map(String::from).to_vec()),
            completion_item: Some(CompletionOptionsCompletionItem {
                label_details_support: Some(support.label_details),
            }),
            ..Default::default()
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
        implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
        references_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        // Spec 25. Lenses are advertised unconditionally: they are discovery,
        // and a bench with no site still shows what is runnable and why it is
        // not.
        code_lens_provider: Some(CodeLensOptions {
            resolve_provider: Some(false),
        }),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec![RUN_TEST.to_string(), SELECT_SITE.to_string()],
            ..Default::default()
        }),
        // Push via `publishDiagnostics` is the fallback, so the pull provider is
        // advertised only to clients that asked for it.
        diagnostic_provider: support.pull_diagnostics.then(|| {
            DiagnosticServerCapabilities::Options(DiagnosticOptions {
                identifier: Some(DIAGNOSTIC_SOURCE.to_string()),
                // A DocType JSON edit changes results in every file citing it.
                inter_file_dependencies: true,
                workspace_diagnostics: false,
                ..Default::default()
            })
        }),
        workspace: Some(WorkspaceServerCapabilities {
            workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                supported: Some(true),
                change_notifications: Some(OneOf::Left(true)),
            }),
            file_operations: None,
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::ls_types::{
        DiagnosticClientCapabilities, GeneralClientCapabilities, GotoCapability,
        PositionEncodingKind, TextDocumentClientCapabilities,
    };

    #[test]
    fn a_bare_client_gets_the_documented_fallbacks() {
        let support = ClientSupport::detect(&ClientCapabilities::default());

        assert_eq!(support.position_encoding, PositionEncoding::Utf16);
        assert!(!support.location_link);
        assert!(!support.markdown);
        assert!(!support.pull_diagnostics);

        let capabilities = server_capabilities(support);
        assert!(capabilities.diagnostic_provider.is_none());
        assert_eq!(
            capabilities.position_encoding,
            Some(PositionEncodingKind::UTF16)
        );
    }

    #[test]
    fn utf8_is_used_when_the_client_offers_it() {
        let support = ClientSupport::detect(&ClientCapabilities {
            general: Some(GeneralClientCapabilities {
                position_encodings: Some(vec![
                    PositionEncodingKind::UTF8,
                    PositionEncodingKind::UTF16,
                ]),
                ..Default::default()
            }),
            ..Default::default()
        });

        assert_eq!(support.position_encoding, PositionEncoding::Utf8);
    }

    #[test]
    fn pull_diagnostics_are_advertised_only_on_request() {
        let support = ClientSupport::detect(&ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                diagnostic: Some(DiagnosticClientCapabilities::default()),
                definition: Some(GotoCapability {
                    link_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        });

        assert!(support.pull_diagnostics);
        assert!(support.location_link);
        assert!(server_capabilities(support).diagnostic_provider.is_some());
    }

    #[test]
    fn sync_is_incremental() {
        let capabilities = server_capabilities(ClientSupport::default());
        let Some(TextDocumentSyncCapability::Options(options)) = capabilities.text_document_sync
        else {
            panic!("expected sync options");
        };

        assert_eq!(options.change, Some(TextDocumentSyncKind::INCREMENTAL));
        assert_eq!(options.open_close, Some(true));
    }
}
