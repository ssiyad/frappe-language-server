-- End to end in a real editor: stock Neovim (0.11+), built-in LSP client, no
-- plugins, driving the release binary over a real bench.
--
--   cargo build --release
--   nvim --clean --headless -l tests/editors/neovim.lua [BENCH]
--
-- It loads the plugin from this repository the way a plugin manager would,
-- so the plugin users install is the configuration under test. Targets are found by searching
-- the buffer rather than by line number, so the check survives the bench
-- moving. Exits non-zero on the first broken expectation.

local repo = vim.fs.dirname(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(arg[0]))))
local bench = vim.fs.normalize(arg[1] or os.getenv('FRAPPE_LSP_REFERENCE_BENCH') or '~/sources/frappe-dev')
local binary = os.getenv('FRAPPE_LSP') or (repo .. '/target/release/frappe-lsp')

-- The checks run against either bench: the reference bench with erpnext, or
-- this repository's fixture bench, whose `library` app shapes the same
-- patterns on purpose. CI runs the fixture; a real bench stays the default.
local erpnext = vim.uv.fs_stat(bench .. '/apps/erpnext') ~= nil
local T = erpnext and {
  controller = 'apps/erpnext/erpnext/selling/doctype/sales_order/sales_order.py',
  get_doc = { 'frappe.get_doc("Quotation"', 17, 'Quotation' },
  controller_target = 'quotation/quotation.py',
  references_min = 20,
  field = { '"Sales Order Item", item.name, "supplier"', 33, 'supplier' },
  typed_line = 'x = frappe.get_doc("Sales Inv', typed_name = 'Sales Invoice',
  form_script = 'apps/erpnext/erpnext/selling/doctype/sales_order/sales_order.js',
  set_value = { 'frm.set_value("delivery_date"', 15, 'delivery_date' },
  method_pattern = 'method: "()(erpnext%.[%w_.]+)"', method_prefix = 'erpnext.',
  outline = 'apps/erpnext/erpnext/selling/doctype/sales_order/sales_order.json',
  hooks = 'apps/erpnext/erpnext/hooks.py',
  hook_path = { 'boot_session = "erpnext.', 18 },
  test_file = 'apps/erpnext/erpnext/selling/doctype/sales_order/test_sales_order.py',
} or {
  controller = 'apps/library/library/api.py',
  get_doc = { 'frappe.get_doc("Library Member"', 17, 'Library Member' },
  controller_target = 'library_member.py',
  references_min = 5,
  field = { '"Library Member", member, "loyalty_points"', 27, 'loyalty_points' },
  typed_line = 'x = frappe.get_doc("Library Memb', typed_name = 'Library Member',
  form_script = 'apps/library/library/library/doctype/library_member/library_member.js',
  set_value = { 'frm.set_value("email_id"', 15, 'email_id' },
  method_pattern = 'method: "()(library%.[%w_.]+)"', method_prefix = 'library.',
  outline = 'apps/library/library/library/doctype/library_member/library_member.json',
  hooks = 'apps/library/library/hooks.py',
  hook_path = { 'override_doctype_class = {"Library Member": "library.', 47 },
  test_file = 'apps/library/library/library/doctype/library_member/test_library_member.py',
}

-- The plugin in this repository, as lazy.nvim or vim.pack would load it.
vim.opt.runtimepath:prepend(repo)
vim.g.frappe_lsp_cmd = binary
require('frappe_lsp').setup()

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

-- Python: a controller in the bench's main app.
local buf, client = open(T.controller)
local published = vim.wait(120000, function()
  local status = client:request_sync('frappe-lsp/indexStatus', vim.NIL, 5000, buf)
  return status and status.result and status.result.confidence ~= 'partial'
end, 200)
check('the index publishes', published)

cursor(buf, T.get_doc[1], T.get_doc[2])
local hover, err = request(client, buf, 'textDocument/hover')
local text = hover and hover.contents and (hover.contents.value or hover.contents) or ''
check('hover on a DocType name', type(text) == 'string' and text:find(T.get_doc[3], 1, true) ~= nil, err or text:sub(1, 60))

local definition = request(client, buf, 'textDocument/definition')
local target = definition and definition[1] and (definition[1].targetUri or definition[1].uri) or ''
check('definition lands on the controller', target:find(T.controller_target, 1, true) ~= nil, target)

local references = request(client, buf, 'textDocument/references', { context = { includeDeclaration = true } })
check('references span the bench', references and #references > T.references_min, references and (#references .. ' sites'))

cursor(buf, T.field[1], T.field[2])
local completion = request(client, buf, 'textDocument/completion')
local seen, count = labels(completion)
check('completion offers the child DocType\'s fields', seen[T.field[3]] == true, count .. ' items')


-- Typing: an unfinished call must still complete, and the server must survive it.
local last = vim.api.nvim_buf_line_count(buf)
vim.api.nvim_buf_set_lines(buf, last, last, false, { T.typed_line })
vim.api.nvim_win_set_cursor(0, { last + 1, #T.typed_line })
seen, count = labels(request(client, buf, 'textDocument/completion'))
check('completion while typing a DocType name', seen[T.typed_name] == true, count .. ' items')

local pulled = client:request_sync('textDocument/diagnostic', { textDocument = vim.lsp.util.make_text_document_params(buf) }, 10000, buf)
check('pull diagnostics answer', pulled and pulled.result ~= nil and pulled.err == nil)
vim.cmd('silent! undo')

-- JavaScript: a form script.
buf, client = open(T.form_script)
cursor(buf, T.set_value[1], T.set_value[2])
seen, count = labels(request(client, buf, 'textDocument/completion'))
check('completion in a form script offers the form\'s fields', seen[T.set_value[3]] == true, count .. ' items')
hover, err = request(client, buf, 'textDocument/hover')
text = hover and hover.contents and (hover.contents.value or hover.contents) or ''
check('hover on a fieldname', type(text) == 'string' and text:find(T.set_value[3], 1, true) ~= nil, err or text:sub(1, 60))

-- A whitelisted path in frappe.call completes from the bench's methods.
for number, line in ipairs(vim.api.nvim_buf_get_lines(buf, 0, -1, false)) do
  local column, path = line:match(T.method_pattern)
  if path then
    vim.api.nvim_win_set_cursor(0, { number, column - 1 + #T.method_prefix })
    seen, count = labels(request(client, buf, 'textDocument/completion'))
    check('completion of a whitelisted method path', seen[path] == true, count .. ' items, wanted ' .. path)
    break
  end
end

-- A DocType JSON has an outline: sections, then their fields.
buf, client = open(T.outline)
local symbols = client:request_sync('textDocument/documentSymbol', { textDocument = vim.lsp.util.make_text_document_params(buf) }, 10000, buf)
local outline = symbols and symbols.result or {}
check('a DocType JSON has an outline', #outline > 0, #outline .. ' top-level')

-- hooks.py: a dotted path goes to its definition.
buf, client = open(T.hooks)
cursor(buf, T.hook_path[1], T.hook_path[2])
definition = request(client, buf, 'textDocument/definition')
target = definition and definition[1] and (definition[1].targetUri or definition[1].uri) or ''
check('definition from a hook path', target:find('.py', 1, true) ~= nil, target)

-- A test file carries run lenses.
buf, client = open(T.test_file)
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
check('the commands exist', vim.fn.exists(':FrappeLspStatus') == 2 and vim.fn.exists(':FrappeLspInstall') == 2)

check('the server is still running', not client:is_stopped())
print(failed == 0 and 'neovim: all checks passed' or ('neovim: ' .. failed .. ' failed'))
vim.cmd((failed == 0 and 'qall!' or 'cquit! 1'))
