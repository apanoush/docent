-- Open a docent answer in Neovim for the zathura adapter.
local request_path = vim.env.DOCENT_ZATHURA_REQUEST

local function fail(message)
  vim.notify("docent-zathura: " .. message, vim.log.levels.ERROR)
end

local function cleanup()
  local dir = vim.fn.fnamemodify(request_path, ":h")
  if vim.fn.fnamemodify(dir, ":t"):match("^docent%-zathura%-") then
    vim.fn.delete(dir, "rf")
  else
    vim.fn.delete(request_path)
  end
end

if not request_path or request_path == "" then
  fail("missing request file")
  return
end

local file = io.open(request_path, "r")
if not file then
  fail("could not read " .. request_path)
  cleanup()
  return
end

local raw = file:read("*a")
file:close()
cleanup()

local ok, payload = pcall(vim.json.decode, raw)
if not ok or type(payload) ~= "table" then
  fail("invalid request")
  return
end

local request = payload.request
if type(request) ~= "table" then
  fail("missing request data")
  return
end

local plugin_ok, docent = pcall(require, "docent")
if not plugin_ok then
  fail("could not load the docent Neovim plugin: " .. tostring(docent))
  return
end

local options = {
  selection = type(request.selection) == "string" and request.selection or "",
  document = type(request.document) == "string" and request.document or "",
  file = type(request.file) == "string" and request.file or "[No Name]",
}
if type(request.byte_start) == "number" then
  options.byte_start = request.byte_start
end
if type(request.byte_end) == "number" then
  options.byte_end = request.byte_end
end
if type(request.question) == "string" and request.question ~= "" then
  options.question = request.question
end
if type(payload.target_language) == "string" and payload.target_language ~= "" then
  options.target_language = payload.target_language
end
if type(payload.context_mode) == "string" and payload.context_mode ~= "" then
  options.context_mode = payload.context_mode
end

local config = vim.deepcopy(docent.config or {})
local resolved = type(payload.docent) == "string" and payload.docent or nil
if resolved then
  if type(config.cmd) == "table" then
    local cmd = vim.deepcopy(config.cmd)
    cmd[1] = resolved
    config.cmd = cmd
  else
    config.cmd = resolved
  end
elseif not config.cmd then
  config.cmd = "docent"
end
config.source = "zathura"
docent.setup(config)
docent.run(type(payload.profile) == "string" and payload.profile or "explain", options)
