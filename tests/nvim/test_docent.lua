-- Headless tests for the docent Neovim plugin.
vim.opt.runtimepath:prepend(vim.fn.getcwd() .. "/nvim")

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
    },
  })

  assert_eq(vim.fn.exists(":Docent"), 2)
  assert_eq(vim.fn.exists(":DocentProfiles"), 2)
  assert_true(vim.fn.maparg("<F5>", "v") ~= "")
  assert_true(vim.fn.maparg("<F6>", "n") ~= "")

  vim.keymap.del("v", "<F5>")
  vim.keymap.del("n", "<F6>")
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
        assert_eq(captured.cmd[2], "run")
        assert_eq(captured.cmd[3], "--profile")
        assert_eq(captured.cmd[4], "explain")
        assert_true(vim.tbl_contains(captured.cmd, "--stream"))

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
