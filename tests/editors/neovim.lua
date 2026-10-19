-- End to end in a real editor: stock Neovim (0.11+), built-in LSP client, no
-- plugins, driving the release binary over a real bench.
--
--   cargo build --release
--   nvim --clean --headless -l tests/editors/neovim.lua [BENCH]
--
-- The setup block below is the one docs/editors.md tells users to copy, so the
-- documented configuration is the tested one. Targets are found by searching
-- the buffer rather than by line number, so the check survives the bench
-- moving. Exits non-zero on the first broken expectation.

local repo = vim.fs.dirname(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(arg[0]))))
local bench = vim.fs.normalize(arg[1] or os.getenv('FRAPPE_LSP_REFERENCE_BENCH') or '~/sources/frappe-dev')
local binary = os.getenv('FRAPPE_LSP') or (repo .. '/target/release/frappe-lsp')

-- >>> setup
vim.lsp.config('frappe_lsp', {
  cmd = { binary },
  filetypes = { 'python', 'javascript', 'json', 'text' },
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
-- <<< setup

local failed = 0
local function check(name, ok, detail)
  print(string.format('%s  %s%s', ok and 'ok  ' or 'FAIL', name, detail and ('  — ' .. detail) or ''))
  if not ok then
    failed = failed + 1
  end
end

local function open(relative)
  vim.cmd.edit(vim.fn.fnameescape(bench .. '/' .. relative))
  local buf = vim.api.nvim_get_current_buf()
  local attached = vim.wait(10000, function()
    return #vim.lsp.get_clients({ bufnr = buf, name = 'frappe_lsp' }) > 0
  end, 50)
  check('attaches to ' .. relative, attached)
  return buf, vim.lsp.get_clients({ bufnr = buf, name = 'frappe_lsp' })[1]
end

-- Put the cursor `offset` bytes into the first match of `needle`.
local function cursor(buf, needle, offset)
  for number, line in ipairs(vim.api.nvim_buf_get_lines(buf, 0, -1, false)) do
    local column = line:find(needle, 1, true)
    if column then
      vim.api.nvim_win_set_cursor(0, { number, column - 1 + (offset or 0) })
      return true
    end
  end
  error('not in the buffer: ' .. needle)
end

local function request(client, buf, method, extra)
  local params = vim.lsp.util.make_position_params(0, client.offset_encoding)
  for key, value in pairs(extra or {}) do
    params[key] = value
  end
  local response = client:request_sync(method, params, 10000, buf)
  if not response then
    return nil, 'no response'
  end
  if response.err then
    return nil, vim.inspect(response.err)
  end
  return response.result
end

local function labels(result)
  local items = result and (result.items or result) or {}
  local out = {}
  for _, item in ipairs(items) do
    out[item.label] = true
  end
  return out, #items
end

-- Python: a controller in erpnext.
local buf, client = open('apps/erpnext/erpnext/selling/doctype/sales_order/sales_order.py')
local published = vim.wait(120000, function()
  local status = client:request_sync('frappe-lsp/indexStatus', vim.NIL, 5000, buf)
  return status and status.result and status.result.confidence ~= 'partial'
end, 200)
check('the index publishes', published)

cursor(buf, 'frappe.get_doc("Quotation"', 17)
local hover, err = request(client, buf, 'textDocument/hover')
local text = hover and hover.contents and (hover.contents.value or hover.contents) or ''
check('hover on a DocType name', type(text) == 'string' and text:find('Quotation', 1, true) ~= nil, err or text:sub(1, 60))

local definition = request(client, buf, 'textDocument/definition')
local target = definition and definition[1] and (definition[1].targetUri or definition[1].uri) or ''
check('definition lands on the controller', target:find('quotation/quotation.py', 1, true) ~= nil, target)

local references = request(client, buf, 'textDocument/references', { context = { includeDeclaration = true } })
check('references span the bench', references and #references > 20, references and (#references .. ' sites'))

cursor(buf, '"Sales Order Item", item.name, "supplier"', 33)
local completion = request(client, buf, 'textDocument/completion')
local seen, count = labels(completion)
check('completion offers the child DocType\'s fields', seen['supplier'] == true, count .. ' items')


-- Typing: an unfinished call must still complete, and the server must survive it.
local last = vim.api.nvim_buf_line_count(buf)
vim.api.nvim_buf_set_lines(buf, last, last, false, { 'x = frappe.get_doc("Sales Inv' })
vim.api.nvim_win_set_cursor(0, { last + 1, #'x = frappe.get_doc("Sales Inv' })
seen, count = labels(request(client, buf, 'textDocument/completion'))
check('completion while typing a DocType name', seen['Sales Invoice'] == true, count .. ' items')

local pulled = client:request_sync('textDocument/diagnostic', { textDocument = vim.lsp.util.make_text_document_params(buf) }, 10000, buf)
check('pull diagnostics answer', pulled and pulled.result ~= nil and pulled.err == nil)
vim.cmd('silent! undo')

-- JavaScript: a form script.
buf, client = open('apps/erpnext/erpnext/selling/doctype/sales_order/sales_order.js')
cursor(buf, 'frm.set_value("delivery_date"', 15)
seen, count = labels(request(client, buf, 'textDocument/completion'))
check('completion in a form script offers the form\'s fields', seen['delivery_date'] == true, count .. ' items')
hover, err = request(client, buf, 'textDocument/hover')
text = hover and hover.contents and (hover.contents.value or hover.contents) or ''
check('hover on a fieldname', type(text) == 'string' and text:find('delivery_date', 1, true) ~= nil, err or text:sub(1, 60))

-- A whitelisted path in frappe.call completes from the bench's methods.
for number, line in ipairs(vim.api.nvim_buf_get_lines(buf, 0, -1, false)) do
  local column, path = line:match('method: "()(erpnext%.[%w_.]+)"')
  if path then
    vim.api.nvim_win_set_cursor(0, { number, column - 1 + #'erpnext.' })
    seen, count = labels(request(client, buf, 'textDocument/completion'))
    check('completion of a whitelisted method path', seen[path] == true, count .. ' items, wanted ' .. path)
    break
  end
end

-- A DocType JSON has an outline: sections, then their fields.
buf, client = open('apps/erpnext/erpnext/selling/doctype/sales_order/sales_order.json')
local symbols = client:request_sync('textDocument/documentSymbol', { textDocument = vim.lsp.util.make_text_document_params(buf) }, 10000, buf)
local outline = symbols and symbols.result or {}
check('a DocType JSON has an outline', #outline > 0, #outline .. ' top-level')

-- hooks.py: a dotted path goes to its definition.
buf, client = open('apps/erpnext/erpnext/hooks.py')
cursor(buf, 'boot_session = "erpnext.', 18)
definition = request(client, buf, 'textDocument/definition')
target = definition and definition[1] and (definition[1].targetUri or definition[1].uri) or ''
check('definition from a hook path', target:find('.py', 1, true) ~= nil, target)

-- A test file carries run lenses.
buf, client = open('apps/erpnext/erpnext/selling/doctype/sales_order/test_sales_order.py')
local lenses = client:request_sync('textDocument/codeLens', { textDocument = vim.lsp.util.make_text_document_params(buf) }, 10000, buf)
local lens_count = lenses and lenses.result and #lenses.result or 0
check('test lenses', lens_count > 0, lens_count .. ' lenses')

-- What a click sends. Not run: it would execute erpnext's tests against a site.
-- A bench with several sites asks which one; headless, nobody can answer.
vim.lsp.handlers['window/showMessageRequest'] = function(_, params)
  return params.actions and params.actions[1] or vim.NIL
end
local function first_lens()
  local response = client:request_sync('textDocument/codeLens', { textDocument = vim.lsp.util.make_text_document_params(buf) }, 10000, buf)
  return response and response.result and response.result[1]
end
local lens = first_lens()
-- On a bench with several sites the first click picks one; the lenses that
-- come back after it carry the command that runs.
if lens and lens.command and lens.command.command == 'frappe-lsp.selectSite' then
  client:request_sync('workspace/executeCommand', lens.command, 10000, buf)
  lens = first_lens()
end
local run = lens and lens.command and client:request_sync('workspace/executeCommand', lens.command, 10000, buf)
local argv = run and run.result and run.result.argv or {}
check('a lens resolves to a bench command', vim.tbl_contains(argv, 'run-tests'), table.concat(argv, ' '))
check('the runTest handler is registered', type(vim.lsp.commands['frappe-lsp.runTest']) == 'function')

check('the server is still running', not client:is_stopped())
print(failed == 0 and 'neovim: all checks passed' or ('neovim: ' .. failed .. ' failed'))
vim.cmd((failed == 0 and 'qall!' or 'cquit! 1'))
