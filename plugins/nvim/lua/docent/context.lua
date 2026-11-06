-- Buffer capture helpers for docent.nvim.
local M = {}

local function is_visual_mode(mode)
  return mode == "v" or mode == "V" or mode == "\22"
end

--- Returns the full buffer text with lines joined by newlines.
function M.buffer_text(buf)
  return table.concat(vim.api.nvim_buf_get_lines(buf, 0, -1, false), "\n")
end

--- Returns a display name for the buffer.
function M.file_name(buf)
  local name = vim.api.nvim_buf_get_name(buf)
  if name == "" then
    return "[No Name]"
  end
  return name
end

--- Returns the byte offset of a 0-based column on a 1-based line.
function M.byte_offset(buf, line, col)
  return vim.api.nvim_buf_get_offset(buf, line - 1) + col
end

--- Captures a line range as a request fragment.
function M.capture_range(buf, line1, line2)
  local line_count = vim.api.nvim_buf_line_count(buf)
  line1 = math.max(1, math.min(line1, line_count))
  line2 = math.max(1, math.min(line2, line_count))
  if line1 > line2 then
    line1, line2 = line2, line1
  end

  local lines = vim.api.nvim_buf_get_lines(buf, line1 - 1, line2, false)
  local selection = table.concat(lines, "\n")
  local start_byte = M.byte_offset(buf, line1, 0)

  return {
    selection = selection,
    document = M.buffer_text(buf),
    byte_start = start_byte,
    byte_end = start_byte + #selection,
    file = M.file_name(buf),
  }
end

--- Captures the current line.
function M.capture_line(buf)
  local line = vim.api.nvim_win_get_cursor(0)[1]
  return M.capture_range(buf, line, line)
end

--- Captures the active visual selection.
function M.capture_visual(buf)
  local mode = vim.fn.mode()
  if not is_visual_mode(mode) then
    return M.capture_line(buf)
  end

  local anchor = vim.fn.getpos("v")
  local cursor = vim.fn.getpos(".")
  local anchor_line = anchor[2]
  local cursor_line = cursor[2]

  if mode == "\22" then
    local line1 = math.min(anchor_line, cursor_line)
    local line2 = math.max(anchor_line, cursor_line)
    return M.capture_range(buf, line1, line2)
  end

  if mode == "V" then
    local line1 = math.min(anchor_line, cursor_line)
    local line2 = math.max(anchor_line, cursor_line)
    return M.capture_range(buf, line1, line2)
  end

  local ok, selection = pcall(vim.fn.getregion, anchor, cursor, { type = mode })
  if not ok then
    local line1 = math.min(anchor_line, cursor_line)
    local line2 = math.max(anchor_line, cursor_line)
    return M.capture_range(buf, line1, line2)
  end

  local anchor_byte = M.byte_offset(buf, anchor_line, anchor[3] - 1)
  local cursor_byte = M.byte_offset(buf, cursor_line, cursor[3] - 1)
  local start_byte = math.min(anchor_byte, cursor_byte)

  return {
    selection = selection,
    document = M.buffer_text(buf),
    byte_start = start_byte,
    byte_end = start_byte + #selection,
    file = M.file_name(buf),
  }
end

--- Builds a request table from a visual selection, a line range, or the current line.
function M.build_request(opts)
  opts = opts or {}

  if opts.selection ~= nil then
    return {
      selection = opts.selection,
      document = opts.document or "",
      byte_start = opts.byte_start,
      byte_end = opts.byte_end,
      file = opts.file or "[No Name]",
    }
  end

  local buf = vim.api.nvim_get_current_buf()
  if opts.range then
    return M.capture_range(buf, opts.range[1], opts.range[2])
  end
  if is_visual_mode(vim.fn.mode()) then
    return M.capture_visual(buf)
  end
  return M.capture_line(buf)
end

M.is_visual_mode = is_visual_mode

return M
