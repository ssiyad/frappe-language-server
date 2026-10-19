# Editor setup

frappe-lsp speaks LSP over stdio. Any editor with an LSP client can run it, and
no extension is needed. Run it beside your Python and JavaScript servers, not
instead of them ([00](specs/00-overview.md)).

The server finds the bench by itself. It walks up from the root the editor
sends until it finds a bench, so an app's own checkout under `apps/` works as a
root ([03](specs/03-workspace-model.md)).

## Neovim

Neovim 0.11 or later, built-in client, no plugins. `tests/editors/neovim.lua`
runs this block against a real bench, so it is the tested configuration.

```lua
vim.lsp.config('frappe_lsp', {
  cmd = { 'frappe-lsp' },
  filetypes = { 'python', 'javascript', 'json', 'text', 'html', 'htmldjango', 'jinja' },
  -- A bench is the directory holding sites/apps.txt.
  root_dir = function(bufnr, on_dir)
    for dir in vim.fs.parents(vim.api.nvim_buf_get_name(bufnr)) do
      if vim.uv.fs_stat(dir .. '/sites/apps.txt') then
        return on_dir(dir)
      end
    end
  end,
})
vim.lsp.enable('frappe_lsp')

-- Test lenses. The server answers with the command to run, and the editor runs
-- it in a terminal so the tests get a TTY.
vim.lsp.commands['frappe-lsp.runTest'] = function(command, ctx)
  local client = assert(vim.lsp.get_client_by_id(ctx.client_id))
  client:request('workspace/executeCommand', command, function(err, result)
    if err or not result or not result.argv then
      vim.notify('frappe-lsp: ' .. (err and err.message or 'nothing to run'), vim.log.levels.WARN)
      return
    end
    vim.cmd('botright new')
    vim.fn.jobstart(result.argv, { cwd = result.cwd, term = true })
  end, ctx.bufnr)
end
```

Neovim does not show code lenses by default. Turn them on with
`vim.lsp.codelens.enable(true)` in 0.12, or call `vim.lsp.codelens.refresh()`
on `BufEnter` and `InsertLeave` in 0.11. Run the lens under the cursor with
`vim.lsp.codelens.run()`.

A bench with more than one site asks which site to run tests against on the
first click. Set `test.site` to skip the question ([30](specs/30-configuration.md)).

## Helix

In `~/.config/helix/languages.toml`. Keep the servers you already use, and add
`frappe-lsp` after them. The names below are examples:

```toml
[language-server.frappe-lsp]
command = "frappe-lsp"

[[language]]
name = "python"
language-servers = ["pyright", "ruff", "frappe-lsp"]

[[language]]
name = "javascript"
language-servers = ["typescript-language-server", "frappe-lsp"]

[[language]]
name = "html"
language-servers = ["vscode-html-language-server", "frappe-lsp"]
```

Helix has no code lens support, so tests do not run from the editor.

## Other editors

Register a stdio server with the command `frappe-lsp` for Python, JavaScript,
JSON and HTML files. HTML files are read as Jinja templates. Then check three things:

1. The client sends `rootUri` or `workspaceFolders`. The server cannot find the
   bench without one of them.
2. The client answers `workspace/executeCommand` for `frappe-lsp.runTest` with
   the reply's `argv` and `cwd`. Otherwise test lenses do nothing.
3. The client keeps its own Python server. frappe-lsp gives no Python or
   JavaScript language features of its own.

## Configuration

A zero-config start against a bench works. To change anything, put a
`.frappe-lsp.toml` in the bench, or a `config.toml` in
`$XDG_CONFIG_HOME/frappe-lsp/`. [30](specs/30-configuration.md) has every key.
For example, to stop a check on one bench:

```toml
[diagnostics]
unknown-form-event = "off"
```

One line can opt out with a comment on it or on the line above:
`frappe-lsp: ignore` for every check, or `frappe-lsp: ignore <code>` for one
([23](specs/23-diagnostics.md)).

## When something is wrong

Send `frappe-lsp/indexStatus` and read the answer first. It reports how many
apps, DocTypes, fields and modules the server found, the frappe version, and
whether the index is complete (`confidence`). Zero apps means it found no bench
above the root the editor sent. In Neovim:

```lua
:lua vim.print(vim.lsp.get_clients({ name = 'frappe_lsp' })[1]:request_sync('frappe-lsp/indexStatus', vim.NIL).result)
```

For more detail, add `--log-level debug` to the command, for example
`cmd = { 'frappe-lsp', '--log-level', 'debug' }`. The server logs to stderr,
and Neovim writes that to `:LspLog`. Attach both to a bug report.
