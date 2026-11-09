-- Headless tests for the docent Neovim plugin.
vim.opt.runtimepath:prepend(vim.fn.getcwd() .. "/plugins/nvim")

local context = require("docent.context")
local docent = require("docent")
local ui_module = require("docent.ui")

local tests = {}
local failures = 0

local function test(name, fn)
  table.insert(tests, { name = name, fn = fn })
end

local function assert_eq(actual, expected, message)
  if actual ~= expected then
    error(
      (message or "assertion failed")
        .. ": expected "
        .. vim.inspect(expected)
        .. ", got "
        .. vim.inspect(actual),
      2
    )
  end
end

local function assert_true(value, message)
  if not value then
    error(message or "expected a truthy value", 2)
  end
end

local function assert_false(value, message)
  if value then
    error(message or "expected a falsy value", 2)
  end
end

local function assert_nil(value, message)
  if value ~= nil then
    error((message or "expected nil") .. ", got " .. vim.inspect(value), 2)
  end
end

test("setup registers commands and keymaps", function()
  docent.setup({
    keymaps = {
      explain = "<F5>",
      pick = "<F6>",
      summarize = "<F7>",
    },
  })

  assert_eq(vim.fn.exists(":Docent"), 2)
  assert_eq(vim.fn.exists(":DocentProfiles"), 2)
  assert_true(vim.fn.maparg("<F5>", "v") ~= "")
  assert_true(vim.fn.maparg("<F6>", "n") ~= "")
  assert_true(vim.fn.maparg("<F7>", "v") ~= "")

  vim.keymap.del("v", "<F5>")
  vim.keymap.del("n", "<F6>")
  vim.keymap.del("v", "<F7>")
end)

test("capture_range returns exact selection and offsets", function()
  local buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "alpha", "beta gamma", "delta" })

  local request = context.capture_range(buf, 2, 2)
  assert_eq(request.selection, "beta gamma")
  assert_eq(request.byte_start, 6)
  assert_eq(request.byte_end, 16)
  assert_eq(request.document, "alpha\nbeta gamma\ndelta")

  vim.api.nvim_buf_delete(buf, { force = true })
end)

test("capture_range clamps and swaps line bounds", function()
  local buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "alpha", "beta" })

  local request = context.capture_range(buf, 99, 0)
  assert_eq(request.selection, "alpha\nbeta")
  assert_eq(request.byte_start, 0)
  assert_eq(request.byte_end, #request.document)

  vim.api.nvim_buf_delete(buf, { force = true })
end)

test("capture_visual returns exact text and offsets for a charwise selection", function()
  local buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "alpha beta", "second line" })
  vim.api.nvim_set_current_buf(buf)
  vim.api.nvim_win_set_cursor(0, { 1, 6 })
  vim.cmd("normal! v")
  vim.api.nvim_win_set_cursor(0, { 1, 9 })

  local request = context.capture_visual(buf)
  assert_eq(request.selection, "beta")
  assert_eq(request.byte_start, 6)
  assert_eq(request.byte_end, 10)
  assert_eq(request.document, "alpha beta\nsecond line")

  vim.cmd("normal! \27")
  vim.api.nvim_buf_delete(buf, { force = true })
end)

test("build_request accepts an explicit selection", function()
  local request = context.build_request({
    selection = "x",
    document = "axb",
    byte_start = 1,
    byte_end = 2,
    file = "notes.md",
  })
  assert_eq(request.selection, "x")
  assert_eq(request.document, "axb")
  assert_eq(request.byte_start, 1)
  assert_eq(request.byte_end, 2)
  assert_eq(request.file, "notes.md")
end)

test("build_request uses the current buffer for a range", function()
  local buf = vim.api.nvim_get_current_buf()
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "one", "two", "three" })

  local request = context.build_request({ range = { 2, 2 } })
  assert_eq(request.selection, "two")
  assert_eq(request.byte_start, 4)
  assert_eq(request.byte_end, 7)
  assert_eq(request.document, "one\ntwo\nthree")
end)

test("build_request can skip the current line", function()
  local buf = vim.api.nvim_get_current_buf()
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "one", "two", "three" })
  vim.api.nvim_win_set_cursor(0, { 2, 0 })

  local selected = context.build_request({})
  assert_eq(selected.selection, "two")

  local document_only = context.build_request({ select_current_line = false })
  assert_eq(document_only.selection, "")
  assert_nil(document_only.byte_start)
  assert_nil(document_only.byte_end)
  assert_eq(document_only.document, "one\ntwo\nthree")
end)

test("ui appends deltas and finishes", function()
  local ui = ui_module.open({ profile = "explain" })
  assert_true(ui:is_open())

  ui:append_text("Hello")
  ui:append_text(" world\nnext")
  local lines = vim.api.nvim_buf_get_lines(ui:buffer(), 0, -1, false)
  assert_eq(table.concat(lines, "\n"), "Hello world\nnext")

  ui:done("test-model")
  assert_true(ui:is_done())
  assert_true(ui:is_open())

  ui:close(false)
  assert_false(ui:is_open())
end)

test("ui shows no output when the stream is empty", function()
  local ui = ui_module.open({ profile = "explain" })
  ui:finish()

  local lines = vim.api.nvim_buf_get_lines(ui:buffer(), 0, -1, false)
  assert_eq(table.concat(lines, "\n"), "(no output)")
  assert_true(ui:is_done())

  ui:close(false)
end)

test("ui renders errors", function()
  local ui = ui_module.open({ profile = "explain" })
  ui:set_error("bad key")

  local lines = vim.api.nvim_buf_get_lines(ui:buffer(), 0, -1, false)
  assert_true(vim.tbl_contains(lines, "error: bad key"))
  assert_true(ui:has_error())
  assert_true(ui:is_done())

  ui:close(false)
end)

test("closing kills the running job", function()
  local ui = ui_module.open({ profile = "explain" })
  local killed = false
  ui:attach_job({
    kill = function()
      killed = true
    end,
  })

  ui:close(true)
  assert_true(killed)
  assert_false(ui:is_open())
end)

test("profiles parses metadata from the CLI", function()
  local original = vim.system
  local calls = {}
  vim.system = function(cmd, _opts)
    table.insert(calls, cmd)
    return {
      wait = function()
        return {
          code = 0,
          stdout = '{"profiles":[{"name":"explain","description":"Explain","requires":["selection"]}]}',
          stderr = "",
        }
      end,
    }
  end

  local ok, err = pcall(function()
    docent.refresh()
    local profiles, error_message = docent.profiles()
    assert_nil(error_message)
    assert_eq(#profiles, 1)
    assert_eq(profiles[1].name, "explain")
    assert_eq(profiles[1].requires[1], "selection")
    assert_eq(calls[1][1], "docent")
    assert_eq(calls[1][2], "profiles")
  end)

  vim.system = original
  if not ok then
    error(err)
  end
end)

local function stub_streaming_run(profiles_json, on_run)
  local original = vim.system
  local captured = {}

  vim.system = function(cmd, opts, on_exit)
    if vim.tbl_contains(cmd, "profiles") then
      return {
        wait = function()
          return { code = 0, stdout = profiles_json, stderr = "" }
        end,
      }
    end

    captured.cmd = cmd
    captured.opts = opts
    captured.killed = false
    local process = {
      kill = function()
        captured.killed = true
      end,
    }

    vim.schedule(function()
      if opts.stdout then
        opts.stdout(
          nil,
          '{"type":"delta","text":"Hello"}\n{"type":"delta","text":" world"}\n{"type":"done","model":"test-model"}\n'
        )
      end
      on_exit({ code = 0, signal = 0, stdout = "", stderr = "" })
    end)

    return process
  end

  on_run(captured)

  vim.system = original
end

test("run streams into a float and builds the CLI command", function()
  docent.setup({ cmd = "docent" })
  docent.refresh()

  local ok, err = pcall(function()
    stub_streaming_run(
      '{"profiles":[{"name":"explain","description":"Explain","requires":["selection"]}]}',
      function(captured)
        docent.run("explain", {
          selection = "beta",
          document = "alpha beta gamma",
          byte_start = 6,
          byte_end = 10,
          file = "notes.md",
        })

        assert_true(vim.wait(500, function()
          return docent._last_ui and docent._last_ui:is_done()
        end))

        local lines = vim.api.nvim_buf_get_lines(docent._last_ui:buffer(), 0, -1, false)
        assert_eq(table.concat(lines, "\n"), "Hello world")
        assert_eq(captured.cmd[1], "docent")
        assert_eq(captured.cmd[2], "request")
        assert_eq(captured.cmd[3], "--profile")
        assert_eq(captured.cmd[4], "explain")
        assert_true(vim.tbl_contains(captured.cmd, "--stream"))
        assert_true(vim.tbl_contains(captured.cmd, "--source"))
        local source_index = 1
        while captured.cmd[source_index] ~= "--source" do
          source_index = source_index + 1
        end
        assert_eq(captured.cmd[source_index + 1], "nvim")

        local request = vim.json.decode(captured.opts.stdin)
        assert_eq(request.selection, "beta")
        assert_eq(request.file, "notes.md")

        docent._last_ui:close(false)
      end
    )
  end)

  if not ok then
    error(err)
  end
end)

test("run records the configured source", function()
  docent.setup({ cmd = "docent", source = "zathura" })
  docent.refresh()

  local ok, err = pcall(function()
    stub_streaming_run(
      '{"profiles":[{"name":"explain","description":"Explain","requires":["selection"]}]}',
      function(captured)
        docent.run("explain", {
          selection = "beta",
          document = "alpha beta gamma",
          file = "notes.md",
        })

        assert_true(vim.wait(500, function()
          return docent._last_ui and docent._last_ui:is_done()
        end))

        local source_index = 1
        while captured.cmd[source_index] ~= "--source" do
          source_index = source_index + 1
        end
        assert_eq(captured.cmd[source_index + 1], "zathura")

        docent._last_ui:close(false)
      end
    )
  end)

  if not ok then
    error(err)
  end
end)

test(":Docent uses a single-line count as a range", function()
  docent.setup({ cmd = "docent" })
  docent.refresh()

  local ok, err = pcall(function()
    stub_streaming_run(
      '{"profiles":[{"name":"explain","requires":["selection"]}]}',
      function(captured)
        local buf = vim.api.nvim_get_current_buf()
        vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "alpha", "beta", "gamma" })
        vim.api.nvim_win_set_cursor(0, { 1, 0 })

        vim.cmd("2Docent explain")
        assert_true(vim.wait(500, function()
          return docent._last_ui and docent._last_ui:is_done()
        end))

        local request = vim.json.decode(captured.opts.stdin)
        assert_eq(request.selection, "beta")
        docent._last_ui:close(false)
      end
    )
  end)

  if not ok then
    error(err)
  end
end)

test("run sends the document alone when no selection is required", function()
  docent.setup({ cmd = "docent" })
  docent.refresh()

  local ok, err = pcall(function()
    stub_streaming_run(
      '{"profiles":[{"name":"summarize","description":"Summarize","requires":["context"]}]}',
      function(captured)
        local buf = vim.api.nvim_get_current_buf()
        vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "alpha", "beta", "gamma" })
        vim.api.nvim_win_set_cursor(0, { 2, 0 })

        docent.run("summarize")

        assert_true(vim.wait(500, function()
          return docent._last_ui and docent._last_ui:is_done()
        end))

        local request = vim.json.decode(captured.opts.stdin)
        assert_eq(request.document, "alpha\nbeta\ngamma")
        assert_eq(request.selection, "")
        assert_nil(request.byte_start)
        assert_nil(request.byte_end)

        docent._last_ui:close(false)
      end
    )
  end)

  if not ok then
    error(err)
  end
end)

test(":Docent summarize sends the whole buffer without a selection", function()
  docent.setup({ cmd = "docent" })
  docent.refresh()

  local ok, err = pcall(function()
    stub_streaming_run(
      '{"profiles":[{"name":"summarize","requires":["context"]}]}',
      function(captured)
        local buf = vim.api.nvim_get_current_buf()
        vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "alpha", "beta", "gamma" })
        vim.api.nvim_win_set_cursor(0, { 1, 0 })

        vim.cmd("Docent summarize")
        assert_true(vim.wait(500, function()
          return docent._last_ui and docent._last_ui:is_done()
        end))

        local request = vim.json.decode(captured.opts.stdin)
        assert_eq(request.document, "alpha\nbeta\ngamma")
        assert_eq(request.selection, "")

        docent._last_ui:close(false)
      end
    )
  end)

  if not ok then
    error(err)
  end
end)

test(":Docent summarize with a count still sends that line", function()
  docent.setup({ cmd = "docent" })
  docent.refresh()

  local ok, err = pcall(function()
    stub_streaming_run(
      '{"profiles":[{"name":"summarize","requires":["context"]}]}',
      function(captured)
        local buf = vim.api.nvim_get_current_buf()
        vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "alpha", "beta", "gamma" })
        vim.api.nvim_win_set_cursor(0, { 1, 0 })

        vim.cmd("2Docent summarize")
        assert_true(vim.wait(500, function()
          return docent._last_ui and docent._last_ui:is_done()
        end))

        local request = vim.json.decode(captured.opts.stdin)
        assert_eq(request.selection, "beta")
        assert_eq(request.document, "alpha\nbeta\ngamma")

        docent._last_ui:close(false)
      end
    )
  end)

  if not ok then
    error(err)
  end
end)

test("run notifies CLI warnings on success", function()
  docent.setup({ cmd = "docent" })
  docent.refresh()

  local original_system = vim.system
  local original_notify = vim.notify
  local notifications = {}

  vim.notify = function(message, level)
    table.insert(notifications, { message = message, level = level })
  end
  vim.system = function(cmd, opts, on_exit)
    if vim.tbl_contains(cmd, "profiles") then
      return {
        wait = function()
          return {
            code = 0,
            stdout = '{"profiles":[{"name":"explain","requires":["selection"]}]}',
            stderr = "",
          }
        end,
      }
    end

    local process = { kill = function() end }
    vim.schedule(function()
      opts.stdout(nil, '{"type":"delta","text":"Hello"}\n')
      on_exit({
        code = 0,
        signal = 0,
        stdout = "",
        stderr = "[WARN  docent::cli] document context truncated to 40 characters\n",
      })
    end)
    return process
  end

  local ok, err = pcall(function()
    docent.run("explain", {
      selection = "beta",
      document = "alpha beta",
      byte_start = 6,
      byte_end = 10,
    })

    assert_true(vim.wait(500, function()
      return docent._last_ui and docent._last_ui:is_done()
    end))

    assert_true(#notifications > 0, "expected a notification")
    assert_eq(notifications[1].level, vim.log.levels.WARN)
    assert_true(
      notifications[1].message:find("document context truncated", 1, true) ~= nil,
      "expected the CLI warning text"
    )

    docent._last_ui:close(false)
  end)

  vim.system = original_system
  vim.notify = original_notify
  if not ok then
    error(err)
  end
end)

test("run surfaces stream error events", function()
  docent.setup({ cmd = "docent" })
  docent.refresh()

  local original = vim.system
  vim.system = function(cmd, opts, on_exit)
    if vim.tbl_contains(cmd, "profiles") then
      return {
        wait = function()
          return {
            code = 0,
            stdout = '{"profiles":[{"name":"explain","requires":["selection"]}]}',
            stderr = "",
          }
        end,
      }
    end

    local process = { kill = function() end }
    vim.schedule(function()
      opts.stdout(nil, '{"type":"error","message":"bad key"}\n')
      on_exit({ code = 3, signal = 0, stdout = "", stderr = "error: bad key\n" })
    end)
    return process
  end

  local ok, err = pcall(function()
    docent.run("explain", {
      selection = "beta",
      document = "alpha beta",
      byte_start = 6,
      byte_end = 10,
    })

    assert_true(vim.wait(500, function()
      return docent._last_ui and docent._last_ui:is_done()
    end))

    local lines = vim.api.nvim_buf_get_lines(docent._last_ui:buffer(), 0, -1, false)
    assert_true(vim.tbl_contains(lines, "error: bad key"))
    assert_true(docent._last_ui:has_error())

    docent._last_ui:close(false)
  end)

  vim.system = original
  if not ok then
    error(err)
  end
end)

test("run prompts for a required question", function()
  docent.setup({ cmd = "docent" })
  docent.refresh()

  local original_input = vim.ui.input
  vim.ui.input = function(_opts, callback)
    callback("Why?")
  end

  local ok, err = pcall(function()
    stub_streaming_run(
      '{"profiles":[{"name":"ask","description":"Ask","requires":["selection","question"]}]}',
      function(captured)
        docent.run("ask", {
          selection = "beta",
          document = "alpha beta gamma",
          byte_start = 6,
          byte_end = 10,
        })

        assert_true(vim.wait(500, function()
          return docent._last_ui and docent._last_ui:is_done()
        end))

        local request = vim.json.decode(captured.opts.stdin)
        assert_eq(request.question, "Why?")
        assert_true(vim.tbl_contains(captured.cmd, "--question"))

        docent._last_ui:close(false)
      end
    )
  end)

  vim.ui.input = original_input
  if not ok then
    error(err)
  end
end)

for _, case in ipairs(tests) do
  local ok, err = pcall(case.fn)
  if ok then
    print("ok " .. case.name)
  else
    failures = failures + 1
    print("FAIL " .. case.name .. "\n" .. tostring(err))
  end
end

if failures > 0 then
  io.stderr:write(string.format("%d test(s) failed\n", failures))
  vim.cmd("cquit 1")
else
  print(string.format("%d tests passed", #tests))
end
