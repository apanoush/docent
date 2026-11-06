-- Floating window used to display an answer while it streams.
local M = {}

local function size(value, total, fallback)
  if type(value) ~= "number" then
    return math.max(1, math.floor(total * fallback))
  end
  if value <= 1 then
    return math.max(1, math.floor(total * value))
  end
  return math.max(1, math.floor(value))
end

--- Opens a scratch float and returns its control object.
function M.open(opts)
  opts = opts or {}
  local profile = opts.profile or "explain"
  local title = opts.title or (" docent: " .. profile .. " ")
  local window_opts = opts.window or {}

  local buf = vim.api.nvim_create_buf(false, true)
  vim.bo[buf].buftype = "nofile"
  vim.bo[buf].bufhidden = "wipe"
  vim.bo[buf].swapfile = false
  vim.bo[buf].filetype = "markdown"
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "Thinking..." })
  vim.bo[buf].modifiable = false

  local width = math.max(1, math.min(size(window_opts.width, vim.o.columns, 0.8), vim.o.columns - 4))
  local height = math.max(1, math.min(size(window_opts.height, vim.o.lines, 0.6), vim.o.lines - 4))
  local row = math.max(0, math.floor((vim.o.lines - height) / 2) - 1)
  local col = math.max(0, math.floor((vim.o.columns - width) / 2))

  local win = vim.api.nvim_open_win(buf, opts.enter ~= false, {
    relative = "editor",
    width = width,
    height = height,
    row = row,
    col = col,
    style = "minimal",
    border = window_opts.border or "rounded",
    title = title,
    title_pos = "center",
  })
  vim.wo[win].wrap = true
  vim.wo[win].linebreak = true
  vim.wo[win].conceallevel = 2
  vim.wo[win].number = false
  vim.wo[win].relativenumber = false
  vim.wo[win].signcolumn = "no"
  vim.wo[win].cursorline = false
  vim.wo[win].spell = false

  local state = {
    buf = buf,
    win = win,
    lines = { "Thinking..." },
    profile = profile,
    start = vim.uv.hrtime(),
    done = false,
    error = false,
    job = nil,
  }

  local api = {}

  local function valid()
    return state.buf ~= nil and vim.api.nvim_buf_is_valid(state.buf)
  end

  local function render()
    if not valid() then
      return
    end
    vim.bo[state.buf].modifiable = true
    vim.api.nvim_buf_set_lines(state.buf, 0, -1, false, state.lines)
    vim.bo[state.buf].modifiable = false
    if state.win and vim.api.nvim_win_is_valid(state.win) then
      local last = vim.api.nvim_buf_line_count(state.buf)
      pcall(vim.api.nvim_win_set_cursor, state.win, { last, 0 })
    end
  end

  local function set_title(new_title)
    state.title = new_title
    if state.win and vim.api.nvim_win_is_valid(state.win) then
      pcall(vim.api.nvim_win_set_config, state.win, {
        title = new_title,
        title_pos = "center",
      })
    end
  end

  local function elapsed()
    return (vim.uv.hrtime() - state.start) / 1e9
  end

  function api:append_text(text)
    if not text or text == "" or not valid() or state.error then
      return
    end
    if #state.lines == 1 and state.lines[1] == "Thinking..." then
      state.lines = { "" }
    end
    local parts = vim.split(text, "\n", { plain = true })
    state.lines[#state.lines] = state.lines[#state.lines] .. parts[1]
    for index = 2, #parts do
      table.insert(state.lines, parts[index])
    end
    render()
  end

  function api:set_error(message)
    if not valid() then
      return
    end
    state.error = true
    state.done = true
    if #state.lines == 1 and state.lines[1] == "Thinking..." then
      state.lines = {}
    end
    if #state.lines > 0 and state.lines[#state.lines] ~= "" then
      table.insert(state.lines, "")
    end

    local first = #state.lines + 1
    local text = tostring(message or "unknown error")
    for line in (text .. "\n"):gmatch("(.-)\n") do
      table.insert(state.lines, "error: " .. line)
    end
    render()

    for index = first, #state.lines do
      pcall(vim.api.nvim_buf_add_highlight, state.buf, -1, "ErrorMsg", index - 1, 0, -1)
    end
    set_title(" docent: error ")
  end

  function api:done(model)
    if not valid() then
      return
    end
    state.done = true
    local label = model and (state.profile .. " - " .. model) or state.profile
    set_title(" docent: " .. label .. string.format(" (%.1fs) ", elapsed()))
  end

  function api:finish()
    if not valid() or state.done then
      return
    end
    state.done = true
    if
      #state.lines == 0
      or (#state.lines == 1 and (state.lines[1] == "" or state.lines[1] == "Thinking..."))
    then
      state.lines = { "(no output)" }
      render()
    end
    set_title(" docent: " .. state.profile .. string.format(" (%.1fs) ", elapsed()))
  end

  function api:attach_job(job)
    state.job = job
  end

  function api:detach_job()
    state.job = nil
  end

  function api:is_open()
    return state.win ~= nil and vim.api.nvim_win_is_valid(state.win)
  end

  function api:is_done()
    return state.done
  end

  function api:has_error()
    return state.error
  end

  function api:buffer()
    return state.buf
  end

  function api:window()
    return state.win
  end

  function api:close(kill)
    if kill and state.job then
      pcall(function()
        state.job:kill("sigterm")
      end)
    end
    state.job = nil
    if state.win and vim.api.nvim_win_is_valid(state.win) then
      pcall(vim.api.nvim_win_close, state.win, true)
    end
    state.win = nil
    if state.buf and vim.api.nvim_buf_is_valid(state.buf) then
      pcall(vim.api.nvim_buf_delete, state.buf, { force = true })
    end
    state.buf = nil
  end

  local function close()
    api:close(true)
  end

  vim.keymap.set("n", "q", close, { buffer = buf, desc = "Docent: close" })
  vim.keymap.set("n", "<Esc>", close, { buffer = buf, desc = "Docent: close" })
  vim.keymap.set("n", "<C-c>", close, { buffer = buf, desc = "Docent: cancel" })

  vim.api.nvim_create_autocmd("BufWipeout", {
    buffer = buf,
    once = true,
    callback = function()
      if state.job then
        pcall(function()
          state.job:kill("sigterm")
        end)
        state.job = nil
      end
    end,
  })

  return api
end

return M
