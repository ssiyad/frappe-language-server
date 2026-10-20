//! frappe-lsp for Zed. Zed starts a language server only through an
//! extension; this one finds `frappe-lsp` on the user's PATH and runs it beside
//! whatever Python and JavaScript servers are already configured.

use zed_extension_api as zed;

struct Frappe;

impl zed::Extension for Frappe {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let command = worktree.which("frappe-lsp").ok_or_else(|| {
            "frappe-lsp is not on PATH: download it from \
             https://github.com/ssiyad/frappe-language-server/releases"
                .to_string()
        })?;
        Ok(zed::Command {
            command,
            args: Vec::new(),
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(Frappe);
