-- frappe-lsp for Neovim 0.11+. Neovim finds this file on 'runtimepath', so
-- with the plugin installed `vim.lsp.enable('frappe_lsp')` starts the server.
-- `require('frappe_lsp').setup()` does that and adds the test runner, code
-- lenses and commands — see docs/editors.md.

local install = require('frappe_lsp.install')

return {
  -- The launcher downloads the pinned release on first use (bin/frappe-lsp).
  cmd = { install.binary() },
  -- HTML is read as Jinja; `text` covers patches.txt.
  filetypes = { 'python', 'javascript', 'json', 'text', 'html', 'htmldjango', 'jinja' },
  -- A bench is the directory holding sites/apps.txt. A file in no bench gets
  -- no client: the server has nothing to say about it. The first bench file
  -- waits for the download, so the client starts on a binary that exists.
  root_dir = function(bufnr, on_dir)
    for dir in vim.fs.parents(vim.api.nvim_buf_get_name(bufnr)) do
      if vim.uv.fs_stat(dir .. '/sites/apps.txt') then
        return install.ensure(function(err)
          if err then
            vim.notify('frappe-lsp: ' .. err, vim.log.levels.ERROR)
          else
            on_dir(dir)
          end
        end)
      end
    end
  end,
}
