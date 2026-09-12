local function run()
  assert(vim.fn.has('nvim-0.11') == 1, 'Neovim 0.11+ is required')
  local editors = vim.env.PROA_TEST_EDITORS
  local fixture = vim.env.PROA_TEST_FIXTURE
  vim.opt.swapfile = false
  vim.opt.runtimepath:prepend(editors .. '/neovim')
  vim.lsp.log.set_level(vim.env.PROA_TEST_LOG_LEVEL or 'warn')
  vim.lsp.config('proa', {
    cmd = { vim.env.PROA_TEST_NODE, editors .. '/lsp/bin/proa-lsp.mjs', '--stdio', '--rust-analyzer', vim.env.PROA_RUST_ANALYZER },
    settings = { ['rust-analyzer'] = {
      checkOnSave = false,
      cargo = { targetDir = vim.fs.normalize(editors .. '/../target') },
    } },
  })
  vim.lsp.enable('proa')
  vim.cmd.edit(vim.fn.fnameescape(fixture))
  vim.bo.filetype = 'rust'
  local bufnr = vim.api.nvim_get_current_buf()
  local original = vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)
  local uri = vim.uri_from_bufnr(bufnr)
  local client
  local function connect()
    assert(vim.wait(30000, function()
      local clients = vim.lsp.get_clients({ bufnr = bufnr, name = 'proa' })
      client = clients[1]
      return client and client.initialized
    end, 50), 'Proa LSP did not initialize; inspect :LspLog')
    assert(#vim.lsp.get_clients({ bufnr = bufnr }) == 1, 'Expected one Rust LSP client')
    assert(client.offset_encoding == 'utf-16', 'Expected negotiated UTF-16 positions')
  end
  connect()
  local function params(needle, offset)
    for index, line in ipairs(vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)) do
      local at = line:find(needle, 1, true)
      if at then
        vim.api.nvim_win_set_cursor(0, { index, at - 1 + (offset or 0) })
        return vim.lsp.util.make_position_params(0, client.offset_encoding)
      end
    end
    error('Could not find ' .. needle)
  end
  local function request(method, arguments)
    local response, err = client:request_sync(method, arguments, 15000, bufnr)
    assert(response, tostring(err))
    assert(not response.err, vim.inspect(response.err))
    return response.result
  end
  local function definition(needle, offset, expected)
    local locations
    -- A synchronous request itself pumps Neovim's event loop. Do not issue it
    -- in vim.wait's predicate: that predicate can re-enter and flood the server.
    for _ = 1, 90 do
      local response = client:request_sync('textDocument/definition', params(needle, offset), 5000, bufnr)
      if response and not response.err and response.result and #response.result > 0 then
        locations = response.result
        break
      end
      vim.wait(1000)
    end
    assert(locations, 'No definition for ' .. needle)
    assert(#locations == 1, vim.inspect(locations))
    local location = locations[1]
    assert(vim.uri_to_fname(location.targetUri or location.uri) == fixture)
    local range = location.targetSelectionRange or location.range
    local line = vim.api.nvim_buf_get_lines(bufnr, range.start.line, range.start.line + 1, false)[1]
    assert(line:sub(range.start.character + 1, range['end'].character) == expected, vim.inspect(location))
  end
  local function replace(text)
    vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, vim.split(text, '\n', { plain = true }))
  end
  local before = table.concat(original, '\n')
  definition('**{a.user_name', 7, 'user_name')
  definition('row.user_name', 6, 'user_name')
  definition('row.note.is_empty', 6, 'note')
  definition('person.user_name.as_str', 9, 'user_name')

  local hover = request('textDocument/hover', params('**{a.user_name', 7))
  assert(hover and hover.contents, 'Rust hover is missing')
  local references_params = params('**{a.user_name', 7)
  references_params.context = { includeDeclaration = false }
  local references = request('textDocument/references', references_params)
  assert(references and #references >= 3, 'Template references are missing')

  local tokens = request('textDocument/semanticTokens/full', { textDocument = { uri = uri } })
  local target = params('**{a.user_name', 5).position
  local legend = client.server_capabilities.semanticTokensProvider.legend.tokenTypes
  local token_line, token_col, field_found = 0, 0, false
  for index = 1, #tokens.data, 5 do
    local dl, dc, length, kind = unpack(tokens.data, index, index + 3)
    token_line = token_line + dl
    token_col = dl == 0 and token_col + dc or dc
    if token_line == target.line and token_col == target.character and length == 9 then
      assert(legend[kind + 1] == 'property', 'Field is not semantically highlighted')
      field_found = true
    end
  end
  assert(field_found, 'No semantic token for the embedded Rust field')
  print('PASS: navigation, hover, references, and semantic tokens')

  local rename_params = params('**{a.user_name', 7)
  rename_params.newName = 'display_name'
  local rename = request('textDocument/rename', rename_params)
  assert(rename and (rename.changes or rename.documentChanges), 'Rename is missing')
  vim.lsp.util.apply_workspace_edit(rename, client.offset_encoding)
  local renamed = table.concat(vim.api.nvim_buf_get_lines(bufnr, 0, -1, false), '\n')
  assert(renamed:find('**{a.display_name.as_str()}**', 1, true))
  assert(renamed:find('pub display_name: String', 1, true))
  print('PASS: rename')
  replace(before)

  replace(before:gsub('a%.user_name%.as_str%(%)', 'a.', 1))
  local completion_params = params('**{a.}', 5)
  completion_params.context = { triggerKind = 2, triggerCharacter = '.' }
  local completion_found = false
  for _ = 1, 20 do
    local completions = request('textDocument/completion', completion_params)
    for _, item in ipairs(completions and (completions.items or completions) or {}) do
      if item.label == 'user_name' then completion_found = true end
    end
    if completion_found then break end
    vim.wait(200)
  end
  assert(completion_found, 'Incomplete-expression field completion is missing')
  print('PASS: incomplete-expression completion')

  -- Multi-byte and astral characters preceding the field must not move targets.
  replace(before:gsub('%*%*{a%.user_name', '🦀 café **{a.user_name', 1))
  definition('**{a.user_name', 7, 'user_name')
  print('PASS: Unicode positions')
  replace(before:gsub('a%.user_name%.as_str%(%)', 'a.note.as_str()', 1))
  definition('**{a.note', 7, 'note')
  print('PASS: unsaved edits')

  -- Restart with the unsaved modification still in the buffer.
  local previous = client.id
  vim.lsp.enable('proa', false)
  assert(vim.wait(10000, function()
    return #vim.lsp.get_clients({ name = 'proa' }) == 0
  end, 50), 'LSP did not stop')
  vim.lsp.enable('proa')
  connect()
  assert(client.id ~= previous, 'LSP did not restart')
  definition('**{a.note', 7, 'note')

  replace(before:gsub('let client =', 'let   client=', 1))
  local edits = request('textDocument/formatting', {
    textDocument = { uri = uri }, options = { tabSize = 4, insertSpaces = true },
  })
  assert(edits and #edits > 0, 'Formatting did not repair unsaved Rust spacing')
  vim.lsp.util.apply_text_edits(edits, bufnr, client.offset_encoding)
  local formatted = table.concat(vim.api.nvim_buf_get_lines(bufnr, 0, -1, false), '\n')
  local raw = before:match('(r###".-"###)')
  assert(formatted:find(raw, 1, true), 'Formatting changed raw Markdown contents')
  assert(formatted:find('let client =', 1, true), 'Rust spacing was not formatted')
  replace(before)
  assert(vim.deep_equal(vim.api.nvim_buf_get_lines(bufnr, 0, -1, false), original))
  vim.lsp.enable('proa', false)
  assert(vim.wait(10000, function()
    return #vim.lsp.get_clients({ name = 'proa' }) == 0
  end, 50))
  print('PASS: Neovim definitions, hover, references, semantic tokens, rename, completion, Unicode, unsaved edits, restart, and formatting; source unchanged')
end

local ok, err = xpcall(run, debug.traceback)
if not ok then
  io.stderr:write(err .. '\n')
  for _, client in ipairs(vim.lsp.get_clients()) do client:stop(true) end
  vim.cmd('cquit 1')
else
  vim.cmd('qa!')
end
