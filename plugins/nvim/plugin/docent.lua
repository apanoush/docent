-- Load docent with defaults so commands work without an explicit setup() call.
if vim.g.loaded_docent then
  return
end
vim.g.loaded_docent = true

if vim.fn.has("nvim-0.10") == 0 then
  vim.notify("docent requires Neovim 0.10 or newer", vim.log.levels.ERROR)
  return
end

require("docent").setup()
