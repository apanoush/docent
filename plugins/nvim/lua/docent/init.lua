-- Docent: context-aware explanations in a Neovim float.
local M = {}

local defaults = {
  cmd = "docent",
  config_dir = nil,
  default_profile = "explain",
  context_mode = nil,
  target_language = nil,
  keymaps = {},
  window = {
    width = 0.8,
    height = 0.6,
    border = "rounded",
  },
}

M.config = vim.deepcopy(defaults)
M._profiles = nil
M._last_ui = nil

local function trim(value)
  return (tostring(value or ""):gsub("^%s+", ""):gsub("%s+$", ""))
end

--- Extracts the warnings the CLI logged to stderr, without env_logger prefixes.
local function stderr_warnings(stderr)
  local warnings = {}
  for line in (stderr or ""):gmatch("[^\r\n]+") do
    if line:sub(1, 5) == "[WARN" then
      local text = line:match("^%[[^%]]*%]%s*(.+)$")
      if text and text ~= "" then
        table.insert(warnings, text)
      end
    end
  end
  return warnings
end

local function command(extra)
  local cmd = type(M.config.cmd) == "table" and vim.deepcopy(M.config.cmd) or { M.config.cmd }
  if M.config.config_dir then
    table.insert(cmd, "--config-dir")
    table.insert(cmd, M.config.config_dir)
  end
  vim.list_extend(cmd, extra)
  table.insert(cmd, "--source")
  table.insert(cmd, "nvim")
  return cmd
end

local function find_profile(profiles, name)
  for _, profile in ipairs(profiles) do
    if profile.name == name then
      return profile
    end
  end
  return nil
end

local function has_requirement(profile, name)
  for _, requirement in ipairs(profile.requires or {}) do
    if requirement == name then
      return true
    end
  end
  return false
end

--- Shows an error without opening a float.
function M.notify_error(message)
  vim.notify("docent: " .. tostring(message), vim.log.levels.ERROR)
end

--- Lists profiles from the CLI, caching the result.
function M.profiles(force)
  if M._profiles and not force then
    return M._profiles
  end

  local cmd = command({ "profiles", "--json" })
  local call_ok, result = pcall(vim.system, cmd, { text = true })
  if not call_ok then
    return nil, "could not run " .. tostring(cmd[1]) .. ": " .. tostring(result)
  end

  local wait_ok, waited = pcall(function()
    return result:wait()
  end)
  if not wait_ok then
    return nil, "failed to run " .. tostring(cmd[1]) .. ": " .. tostring(waited)
  end
  if waited.code ~= 0 then
    local message = trim(waited.stderr)
    if message == "" then
      message = "docent exited with code " .. tostring(waited.code)
    end
    return nil, message
  end

  local decode_ok, decoded = pcall(vim.json.decode, waited.stdout)
  if not decode_ok or type(decoded) ~= "table" or type(decoded.profiles) ~= "table" then
    return nil, "failed to parse profile list"
  end

  M._profiles = decoded.profiles
  return M._profiles
end

--- Clears the cached profile list.
function M.refresh()
  M._profiles = nil
end

--- Returns profile names for command completion.
function M.complete(lead)
  if not M._profiles then
    M.profiles()
  end
  local matches = {}
  for _, profile in ipairs(M._profiles or {}) do
    if profile.name:sub(1, #lead) == lead then
      table.insert(matches, profile.name)
    end
  end
  table.sort(matches)
  return matches
end

--- Opens a picker for available profiles.
function M.pick()
  local profiles, err = M.profiles(true)
  if not profiles then
    M.notify_error(err)
    return
  end
  if #profiles == 0 then
    M.notify_error("no profiles found; run 'docent init'")
    return
  end

  local labels = {}
  local by_label = {}
  for _, profile in ipairs(profiles) do
    local label = profile.name
    if profile.description and profile.description ~= "" then
      label = label .. ": " .. profile.description
    end
    table.insert(labels, label)
    by_label[label] = profile.name
  end

  vim.ui.select(labels, { prompt = "Docent profile" }, function(choice)
    if choice then
      M.run(by_label[choice])
    end
  end)
end

local function start_job(profile, request, opts)
  opts = opts or {}

  local args = { "run", "--profile", profile, "--stream" }
  if request.question and trim(request.question) ~= "" then
    table.insert(args, "--question")
    table.insert(args, request.question)
  end

  local target_language = opts.target_language or M.config.target_language
  if target_language and target_language ~= "" then
    table.insert(args, "--target-language")
    table.insert(args, target_language)
  end

  local context_mode = opts.context_mode or M.config.context_mode
  if context_mode then
    table.insert(args, "--context-mode")
    table.insert(args, context_mode)
  end

  local cmd = command(args)
  local open_ok, ui = pcall(require("docent.ui").open, {
    profile = profile,
    title = " docent: " .. profile .. " ",
    window = M.config.window,
  })
  if not open_ok then
    M.notify_error("could not open window: " .. tostring(ui))
    return
  end
  M._last_ui = ui

  local pending = ""

  local function handle_line(line)
    if line == "" then
      return
    end
    local decode_ok, event = pcall(vim.json.decode, line)
    if not decode_ok or type(event) ~= "table" then
      ui:set_error("invalid stream event")
      return
    end

    if event.type == "delta" then
      ui:append_text(event.text)
    elseif event.type == "done" then
      ui:done(event.model)
    elseif event.type == "error" then
      ui:set_error(event.message)
    end
  end

  local function on_stdout(_, data)
    if not data then
      return
    end
    pending = pending .. data
    while true do
      local newline = pending:find("\n", 1, true)
      if not newline then
        break
      end
      local line = pending:sub(1, newline - 1)
      pending = pending:sub(newline + 1)
      vim.schedule(function()
        handle_line(line)
      end)
    end
  end

  local function on_exit(result)
    vim.schedule(function()
      if pending ~= "" then
        local line = pending
        pending = ""
        handle_line(line)
      end
      ui:detach_job()
      if not ui:is_open() then
        return
      end
      if result.code ~= 0 then
        if not ui:has_error() then
          local message = trim(result.stderr)
          if message == "" then
            message = "docent exited with code " .. tostring(result.code)
          end
          ui:set_error(message)
        end
      else
        for _, warning in ipairs(stderr_warnings(result.stderr)) do
          vim.notify("docent: " .. warning, vim.log.levels.WARN)
        end
        if not ui:is_done() then
          ui:finish()
        end
      end
    end)
  end

  local spawn_ok, job = pcall(vim.system, cmd, {
    stdin = vim.json.encode(request),
    text = true,
    stdout = on_stdout,
  }, on_exit)

  if not spawn_ok then
    ui:set_error("could not start " .. tostring(cmd[1]) .. ": " .. tostring(job))
    return
  end
  ui:attach_job(job)
end

--- Runs a profile against the current selection, line range, or current line.
--- Profiles that do not require a selection receive the document alone.
function M.run(profile, opts)
  profile = profile or M.config.default_profile
  opts = opts or {}

  local profiles, err = M.profiles()
  if not profiles then
    M.notify_error(err)
    return
  end

  local info = find_profile(profiles, profile)
  if not info then
    M.notify_error("unknown profile '" .. profile .. "'")
    return
  end

  local request_opts = vim.tbl_extend("force", {}, opts)
  request_opts.select_current_line = has_requirement(info, "selection")
  local request = require("docent.context").build_request(request_opts)
  if opts.question ~= nil then
    request.question = opts.question
  end

  if has_requirement(info, "selection") and trim(request.selection) == "" then
    M.notify_error("select some text first")
    return
  end
  if has_requirement(info, "context") and trim(request.document) == "" then
    M.notify_error("the document is empty")
    return
  end

  if has_requirement(info, "question") and trim(request.question) == "" then
    vim.ui.input({ prompt = "Question: " }, function(input)
      input = trim(input)
      if input == "" then
        return
      end
      request.question = input
      start_job(profile, request, opts)
    end)
    return
  end

  start_job(profile, request, opts)
end

--- Runs the built-in explain profile.
function M.explain(opts)
  M.run("explain", opts)
end

--- Runs the built-in ask profile.
function M.ask(opts)
  M.run("ask", opts)
end

--- Runs the built-in translate profile.
function M.translate(opts)
  M.run("translate", opts)
end

--- Runs the built-in summarize profile. Without a selection or range it summarizes the buffer.
function M.summarize(opts)
  M.run("summarize", opts)
end

--- Configures the plugin and registers commands and keymaps.
function M.setup(opts)
  M.config = vim.tbl_deep_extend("force", vim.deepcopy(defaults), opts or {})
  M._profiles = nil

  if vim.fn.exists(":Docent") == 0 then
    vim.api.nvim_create_user_command("Docent", function(args)
      local profile = args.args ~= "" and args.args or M.config.default_profile
      local run_opts = {}
      if args.range >= 1 then
        run_opts.range = { args.line1, args.range == 2 and args.line2 or args.line1 }
      end
      M.run(profile, run_opts)
    end, {
      nargs = "?",
      range = true,
      desc = "Docent: explain, ask about, translate, or summarize a passage",
      complete = function(lead)
        return M.complete(lead)
      end,
    })
  end

  if vim.fn.exists(":DocentProfiles") == 0 then
    vim.api.nvim_create_user_command("DocentProfiles", function()
      M.pick()
    end, { desc = "Docent: choose a profile" })
  end

  local maps = M.config.keymaps or {}
  if maps.explain then
    vim.keymap.set("v", maps.explain, function()
      M.run("explain")
    end, { desc = "Docent: explain selection" })
  end
  if maps.ask then
    vim.keymap.set("v", maps.ask, function()
      M.run("ask")
    end, { desc = "Docent: ask about selection" })
  end
  if maps.translate then
    vim.keymap.set("v", maps.translate, function()
      M.run("translate")
    end, { desc = "Docent: translate selection" })
  end
  if maps.summarize then
    vim.keymap.set("v", maps.summarize, function()
      M.run("summarize")
    end, { desc = "Docent: summarize selection" })
  end
  if maps.pick then
    vim.keymap.set("n", maps.pick, function()
      M.pick()
    end, { desc = "Docent: pick profile" })
  end
end

return M
