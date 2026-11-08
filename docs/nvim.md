# Neovim plugin

The plugin captures a selection and its buffer, calls the `docent` CLI asynchronously, and streams the answer into a floating window.

## Requirements

- Neovim 0.10 or newer, because the plugin uses `vim.system` and `getregion`.
- The `docent` binary installed and available on `PATH`. The plugin does not install the CLI.

## Install

With lazy.nvim, point `dir` at the `plugins/nvim/` directory inside this repository:

```lua
{
  dir = "~/path/to/reading-assistant/plugins/nvim",
  config = function()
    require("docent").setup({
      keymaps = {
        explain = "<leader>de",
        ask = "<leader>da",
        translate = "<leader>dt",
        pick = "<leader>dp",
      },
    })
  end,
}
```

For a plain runtimepath setup:

```lua
vim.opt.runtimepath:append("~/path/to/reading-assistant/plugins/nvim")
```

`plugin/docent.lua` calls `require("docent").setup()` with defaults, so `:Docent` and `:DocentProfiles` work even without an explicit `setup()` call. Calling `setup()` with options overrides the defaults and installs keymaps.

Keymaps are opt-in. The plugin never sets a mapping unless you list it in `keymaps`.

## `setup` options

| Option | Type | Default | Meaning |
|---|---|---|---|
| `cmd` | string or string array | `"docent"` | CLI executable and any fixed arguments. A table is useful for a local build or a wrapper. |
| `config_dir` | string | `nil` | Passed to the CLI as `--config-dir`. Leave unset to use `DOCENT_CONFIG_DIR` or the platform config directory. |
| `default_profile` | string | `"explain"` | Profile used by `:Docent` without an argument. |
| `context_mode` | string | `nil` | If set, passed as `--context-mode` for every request. Values are `auto`, `full`, and `none`. |
| `target_language` | string | `nil` | If set, passed as `--target-language` for every request. |
| `keymaps.explain` | string | `nil` | Visual-mode mapping for the explain profile. |
| `keymaps.ask` | string | `nil` | Visual-mode mapping for the ask profile. |
| `keymaps.translate` | string | `nil` | Visual-mode mapping for the translate profile. |
| `keymaps.pick` | string | `nil` | Normal-mode mapping for the profile picker. |
| `window.width` | number | `0.8` | Float width. Values up to 1 are fractions of the editor width; larger values are columns. |
| `window.height` | number | `0.6` | Float height. Values up to 1 are fractions of the editor height; larger values are rows. |
| `window.border` | string | `"rounded"` | Any `nvim_open_win` border style. |

Example with a local binary and a separate config directory:

```lua
require("docent").setup({
  cmd = { vim.fn.expand("~/src/reading-assistant/target/debug/docent") },
  config_dir = vim.fn.expand("~/config/docent-experiments"),
  window = { width = 0.7, height = 0.5, border = "single" },
})
```

## Commands

| Command | Action |
|---|---|
| `:Docent [profile]` | Run a profile against the visual selection, the given range, or the current line. |
| `:DocentProfiles` | Open a profile picker with descriptions from `docent profiles --json`. |

`:Docent` accepts a range, so `:'<,'>Docent explain` uses the selected lines. Without a range it uses the current line. Because the summarize profile needs no selection, use `:%Docent summarize` to summarize the whole buffer, or select a region and run `:'<,'>Docent summarize` for just that region.

## What gets captured

- Charwise visual selections are captured exactly.
- Linewise visual selections capture the full lines.
- Blockwise visual selections capture the full line span, not only the rectangular block.
- Normal-mode calls, including `:Docent` without a range, capture the current line.
- The request includes the full buffer, byte offsets for the selection, and the buffer name. Unnamed buffers use `[No Name]`.

The CLI decides how much of the buffer to send. You can set `context_mode = "none"` for a selection-only request or `"full"` to always send the whole document.

## Profiles and questions

Profiles that declare `requires = ["question"]` cause the plugin to open `vim.ui.input` before starting the request. This works for any profile, not only the built-in ask profile. Profiles that require `selection` fail early with a notification when the selection is empty.

The plugin caches profile metadata for the session. Add or edit a profile, then run `:DocentProfiles` or call `require("docent").refresh()` to refresh the cache.

## Float behavior

- The float opens immediately and receives streamed text as it arrives.
- The float is focused when it opens. Press `q`, `<Esc>`, or `<C-c>` to close it.
- Closing while the request is running sends `SIGTERM` to the CLI.
- The title shows the profile, the model, and the elapsed time when the answer completes.
- Errors are shown in the float with the `ErrorMsg` highlight. Preflight errors such as an empty selection or a missing profile are reported with `vim.notify`.

## Lua API

```lua
local docent = require("docent")

docent.setup(opts)
docent.run("explain", opts)
docent.explain(opts)
docent.ask(opts)
docent.translate(opts)
docent.pick()
docent.profiles(force)
docent.refresh()
```

`run` accepts these options:

| Option | Meaning |
|---|---|
| `question` | Question for profiles that require one. |
| `target_language` | Translation target for this request. |
| `context_mode` | Context mode for this request. |
| `range` | `{ line1, line2 }` to capture full lines. |
| `selection`, `document`, `byte_start`, `byte_end`, `file` | Explicit request data, useful for scripts and tests. |

Example from Lua:

```lua
require("docent").run("ask", {
  question = "What does this argument assume?",
})
```

The plugin does not parse `config.toml`; all configuration and profile loading happens in the CLI. This keeps one source of truth.

## Troubleshooting

- `could not start docent`: the binary is missing from `PATH`. Run `cargo install --path .` or set `cmd` to an absolute path.
- `missing API key`: neither `api_key` nor the environment variable named by `api_key_env` is set. Neovim passes its environment to `vim.system`.
- `no profiles found`: run `docent init`, then `:DocentProfiles`.
- `unknown profile`: the profile file is missing or invalid. Run `docent profiles` in a terminal to see the error.
- No output appears: check `docent run --profile explain --print-prompt` with the same request, then check the provider settings in `config.toml`.
