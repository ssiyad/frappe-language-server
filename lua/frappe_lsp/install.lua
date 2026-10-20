-- The server binary. `bin/frappe-lsp` in this repository downloads the
-- release the plugin pins and runs it, so the plugin needs nothing on PATH.

local M = {}

-- Set when the user names their own command, which is then theirs to provide.
M.custom = false

local installed = nil
local waiting = {}

--- The launcher in this checkout: lua/frappe_lsp/install.lua → bin/frappe-lsp.
function M.launcher()
  local here = debug.getinfo(1, 'S').source:sub(2)
  -- Absolute, so a :cd or the client's cwd cannot break it.
  local root = vim.fs.dirname(vim.fs.dirname(vim.fs.dirname(vim.fn.fnamemodify(here, ':p'))))
  return vim.fs.joinpath(root, 'bin', 'frappe-lsp')
end

--- The command to run: `vim.g.frappe_lsp_cmd`, else the launcher.
function M.binary()
  return vim.g.frappe_lsp_cmd or M.launcher()
end

--- Download the pinned release if it is not there yet, then `done(err, path)`
--- on the main loop. A download says so once; a cached binary is silent.
function M.ensure(done)
  if M.custom or vim.g.frappe_lsp_cmd then
    return done(nil, M.binary())
  end
  if installed then
    return done(nil, installed)
  end
  table.insert(waiting, done)
  if #waiting > 1 then
    return
  end

  local said = false
  local log = {}
  local function stderr(_, data)
    if not data then
      return
    end
    table.insert(log, data)
    local line = data:match('downloading [^\n]*')
    if line and not said then
      said = true
      vim.schedule(function()
        vim.notify('frappe-lsp: ' .. line .. '…')
      end)
    end
  end

  vim.system({ M.launcher(), '--install' }, { text = true, stderr = stderr }, function(result)
    vim.schedule(function()
      local lines = vim.split(vim.trim(table.concat(log)), '\n')
      local err = result.code ~= 0 and lines[#lines] or nil
      local path = vim.trim(result.stdout or '')
      installed = not err and path or nil
      if said and not err then
        vim.notify('frappe-lsp: installed ' .. path)
      end
      local callbacks = waiting
      waiting = {}
      for _, callback in ipairs(callbacks) do
        callback(err, path)
      end
    end)
  end)
end

return M
