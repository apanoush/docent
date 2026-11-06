# Neovim plugin modules

The plugin source lives in `plugins/nvim/`, separate from the Rust crate. It has no Lua dependencies.

## File map

| File | Responsibility |
|---|---|
| `plugin/docent.lua` | Load guard, Neovim version check, and the automatic `setup()` call. |
| `lua/docent/init.lua` | Configuration, commands, keymaps, profile cache, CLI invocation, NDJSON parsing, and the public Lua API. |
| `lua/docent/context.lua` | Buffer text, selection capture, byte offsets, and request construction. |
| `lua/docent/ui.lua` | Scratch buffer and floating window, incremental rendering, titles, errors, and cancellation. |

## `docent.init`

`init.lua` merges user options over defaults, registers `:Docent` and `:DocentProfiles`, and installs only the keymaps the user asked for. Commands are registered once; later `setup()` calls update configuration and keymaps without failing on an existing command.

The profile cache calls `docent profiles --json` synchronously. This is a local process without network access, so it is fast enough for command completion and the picker. `M.refresh()` clears the cache, and `M.pick()` refreshes it before showing the menu.

`M.run(profile, opts)` captures the request before doing anything asynchronous. This matters for visual selections: if the plugin waited for a profile lookup or a question prompt, Neovim would already have left visual mode and the selection would be gone.

The CLI command is an argument array, never a shell string:

```text
docent [--config-dir PATH] run --profile NAME --stream [--question TEXT] [--target-language LANG] [--context-mode MODE]
```

The JSON request is passed through `vim.system`'s `stdin` option. No temporary files are used.

## `docent.context`

`context.lua` uses `nvim_buf_get_offset` for byte offsets and `nvim_buf_get_lines` for text. It handles four capture paths:

- Charwise visual selection: `getregion` supplies the exact text, and the smaller of the two cursor byte offsets becomes the start.
- Linewise visual selection: the full line range is captured.
- Blockwise visual selection: the full line span is captured, which is a deliberate simplification.
- Normal mode or a `:Docent` range: the current line or the requested line range is captured.

The document is the whole buffer with lines joined by newlines. The selection is contiguous in that string for charwise and linewise captures, so `byte_end = byte_start + #selection` is exact.

## `docent.ui`

The UI module owns a scratch buffer with `buftype=nofile`, `bufhidden=wipe`, and `filetype=markdown`. The floating window is opened with `relative=editor`, a centered position, and the configured border and size.

Streaming updates append text to an internal line table and replace the buffer lines. The cursor follows the last line while the answer arrives. Errors append `error: ...` lines and apply the `ErrorMsg` highlight.

The control object exposes `append_text`, `set_error`, `done`, `finish`, `attach_job`, `detach_job`, `is_open`, `is_done`, `has_error`, `buffer`, `window`, and `close`.

`close(kill)` sends `SIGTERM` to the child when `kill` is true. The buffer-local `q`, `<Esc>`, and `<C-c>` mappings call `close(true)`. A `BufWipeout` autocmd also kills the child if the buffer is removed by another path.

## Scheduling

`vim.system` output callbacks append to a pending string. Only complete lines are decoded with `vim.json.decode`. UI mutations run through `vim.schedule`, so they happen on the main loop rather than inside a libuv callback. The `on_exit` callback also schedules its work.

If the process fails before producing an error event, the plugin shows the trimmed stderr text, or the exit code when stderr is empty.

## Tests

`tests/nvim/test_docent.lua` runs headless with `tests/nvim/run.sh`. It covers:

- Range capture and byte offsets.
- Charwise visual capture, including byte offsets, by entering visual mode in a headless buffer.
- Request construction from explicit selections and ranges.
- UI append, finish, error, and cancellation behavior.
- Profile metadata parsing with a stubbed `vim.system`.
- End-to-end command construction and NDJSON handling with a stubbed stream.
- The generic question prompt for profiles that declare `requires = ["question"]`.

A real end-to-end check still requires a provider: select text, press the configured explain mapping, confirm the float contains the answer, then press `q`.
