-- frappe-lsp for Neovim.
--
--   require('frappe_lsp').setup()
--
-- starts the server for files in a bench, runs test lenses in a terminal, and
-- adds :FrappeLspInstall, :FrappeLspStatus and :FrappeLspReindex.

local install = require('frappe_lsp.install')

local M = {}

local defaults = {
  -- The server command. Unset: the release this plugin pins, downloaded on
  -- first use.
  cmd = nil,
  -- Show test lenses and keep them current.
  codelens = true,
}

local function client(bufnr)
  return vim.lsp.get_clients({ name = 'frappe_lsp', bufnr = bufnr })[1]
    or vim.lsp.get_clients({ name = 'frappe_lsp' })[1]
end

-- The server answers a lens with the command to run; the editor runs it in a
-- terminal so the tests get a TTY.
local function run_test(command, ctx)
  local frappe = assert(vim.lsp.get_client_by_id(ctx.client_id))
  frappe:request('workspace/executeCommand', command, function(err, result)
    if err or not result or not result.argv then
      vim.notify('frappe-lsp: ' .. (err and err.message or 'nothing to run'), vim.log.levels.WARN)
      return
    end
    vim.cmd('botright new')
    vim.fn.jobstart(result.argv, { cwd = result.cwd, term = true })
  end, ctx.bufnr)
end

local function codelens(bufnr)
  if vim.lsp.codelens.enable then
    vim.lsp.codelens.enable(true, { bufnr = bufnr })
    return
  end
  -- Neovim 0.11 has no enable(); refresh where the lenses can change.
  vim.lsp.codelens.refresh({ bufnr = bufnr })
  vim.api.nvim_create_autocmd({ 'BufEnter', 'InsertLeave', 'BufWritePost' }, {
    buffer = bufnr,
    callback = function()
      vim.lsp.codelens.refresh({ bufnr = bufnr })
    end,
  })
end

local function commands()
  -- The download happens by itself on the first bench file. This does it now,
  -- for a machine about to go offline.
  vim.api.nvim_create_user_command('FrappeLspInstall', function()
    install.ensure(function(err, path)
      if err then
        vim.notify('frappe-lsp: ' .. err, vim.log.levels.ERROR)
      else
        vim.notify('frappe-lsp: ' .. path)
      end
    end)
  end, { desc = 'Download the frappe-lsp release this plugin pins' })

  vim.api.nvim_create_user_command('FrappeLspStatus', function()
    local frappe = client(0)
    if not frappe then
      vim.notify('frappe-lsp: not running for this buffer; is it inside a bench?', vim.log.levels.WARN)
      return
    end
    local response = frappe:request_sync('frappe-lsp/indexStatus', vim.NIL, 5000, 0)
    vim.print(response and (response.result or response.err) or 'no answer')
  end, { desc = 'Show what frappe-lsp indexed' })

  vim.api.nvim_create_user_command('FrappeLspReindex', function()
    local frappe = client(0)
    if frappe then
      frappe:request('frappe-lsp/reindex', vim.NIL, function() end, 0)
    end
  end, { desc = 'Rebuild the frappe-lsp index from scratch' })
end

function M.setup(opts)
  local options = vim.tbl_deep_extend('force', defaults, opts or {})
  if options.cmd then
    install.custom = true
    vim.lsp.config('frappe_lsp', { cmd = type(options.cmd) == 'table' and options.cmd or { options.cmd } })
  end

  vim.lsp.commands['frappe-lsp.runTest'] = run_test
  commands()

  if options.codelens then
    vim.api.nvim_create_autocmd('LspAttach', {
      group = vim.api.nvim_create_augroup('frappe_lsp', { clear = true }),
      callback = function(event)
        local attached = vim.lsp.get_client_by_id(event.data.client_id)
        if attached and attached.name == 'frappe_lsp' then
          codelens(event.buf)
        end
      end,
    })
  end

  vim.lsp.enable('frappe_lsp')
end

return M
