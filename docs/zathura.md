# Zathura adapter

The zathura adapter reads the selected text from the system clipboard, extracts the current page and its neighbors with `pdftotext`, calls the `docent` CLI, and streams the answer into a terminal window, a Neovim float, or a qutebrowser window or tab. It is an external adapter, not a zathura plugin: zathura's plugin API only loads document backends, so the integration uses a `zathurarc` `exec` binding.

## Requirements

- zathura with `$FILE` and `$PAGE` expansion in `exec`. The adapter targets zathura 2026.10.4 and the poppler plugin.
- Python 3.10 or newer for the adapter script.
- `pdftotext` from poppler-utils. The adapter is PDF only.
- `docent` installed and available on `PATH`, or in `~/.local/bin`, `~/.cargo/bin`, `/usr/local/bin`, or `/usr/bin`. Use `--docent` for any other location.
- A clipboard tool: `wl-paste` on Wayland, or `xclip` or `xsel` on X11.
- A terminal emulator: `foot`, `kitty`, `alacritty`, or `xterm`, or a command in `DOCENT_TERMINAL` or `--terminal`.
- Neovim 0.10 or newer for `--viewer nvim`.
- qutebrowser 3.7 or newer for `--viewer qutebrowser`.

## Install

Add bindings to `~/.config/zathura/zathurarc`:

```zathurarc
map <C-e> exec "/home/you/path/to/reading-assistant/plugins/zathura/docent-zathura explain $FILE $PAGE"
map <C-a> exec "/home/you/path/to/reading-assistant/plugins/zathura/docent-zathura ask $FILE $PAGE"
map <C-t> exec "/home/you/path/to/reading-assistant/plugins/zathura/docent-zathura translate $FILE $PAGE"
map <C-s> exec "/home/you/path/to/reading-assistant/plugins/zathura/docent-zathura summarize $FILE $PAGE"
```

Quote the whole `exec` command. Zathura's `map` command stores only one argument after the shortcut function, so an unquoted command drops the subcommand and the `$FILE` and `$PAGE` arguments.

To stream into Neovim instead of the terminal, add `--viewer nvim` inside the quotes:

```zathurarc
map <C-e> exec "/home/you/path/to/reading-assistant/plugins/zathura/docent-zathura explain $FILE $PAGE --viewer nvim"
```

To open the answer in qutebrowser, use `--viewer qutebrowser`:

```zathurarc
map <C-e> exec "/home/you/path/to/reading-assistant/plugins/zathura/docent-zathura explain $FILE $PAGE --viewer qutebrowser"
```

Use absolute paths. Zathura launched from a desktop entry may not inherit your shell `PATH` or the API-key environment variable. The adapter searches `~/.local/bin`, `~/.cargo/bin`, `/usr/local/bin`, and `/usr/bin` for `docent`; export the key in the session that starts zathura, as the [qutebrowser guide](qutebrowser.md) describes.

Point the binding at this checkout directly, or symlink the directory so updates apply:

```bash
ln -s /home/you/path/to/reading-assistant/plugins/zathura \
  ~/.config/zathura/plugins/docent
```

Select text with the mouse, then press the key. Zathura has no keyboard text selection, so this is a mouse-first workflow. The answer opens in a terminal window, the Neovim float with `--viewer nvim`, or a qutebrowser page with `--viewer qutebrowser`, and streams as it arrives.

The adapter reads both PRIMARY and CLIPBOARD and uses the text that appears in the extracted page, so it works with either `selection-clipboard` value. Pass `--primary` or `--clipboard` to force a target.

`summarize` is the exception to the mouse-first rule: it needs no selection. Without a matching clipboard selection it summarizes the extracted page window, and with one it summarizes that passage.

## Commands

| Command | Action |
|---|---|
| `docent-zathura explain FILE PAGE` | Explain the selection. |
| `docent-zathura ask FILE PAGE` | Ask a question. The terminal prompts for the question if `--question` is omitted. |
| `docent-zathura translate FILE PAGE` | Translate the selection. |
| `docent-zathura summarize FILE PAGE` | Summarize the page window, or the selection when the clipboard matches it. |
| `docent-zathura do PROFILE FILE PAGE` | Run a custom profile. |

Options:

| Option | Default | Meaning |
|---|---|---|
| `--clipboard` | auto | Force the CLIPBOARD selection. |
| `--primary` | auto | Force the PRIMARY selection. |
| `--terminal NAME` | detected | Terminal emulator for the answer viewer. |
| `--viewer {terminal,nvim,qutebrowser}` | `terminal` | Where to show the answer. |
| `--nvim PATH` | `nvim` | Neovim executable for `--viewer nvim`. |
| `--qutebrowser PATH` | `qutebrowser` | qutebrowser executable for `--viewer qutebrowser`. |
| `--qutebrowser-target TARGET` | `window` | qutebrowser `--target` value: `auto`, `tab`, `tab-bg`, `tab-silent`, `tab-bg-silent`, `window`, or `private-window`. |
| `--qutebrowser-css PATH` | none | CSS file appended after the qutebrowser answer styles. |
| `--docent PATH` | searched | CLI executable or command name. |
| `--context-pages N` | `1` | Pages of context before and after the current page. |
| `--context-mode MODE` | profile or config | `auto`, `full`, or `none`. |
| `--question TEXT` | none | Question for `ask` or `do`; `ask` prompts when omitted. |
| `--target-language LANG`, `--to LANG` | profile or config | Translation target for `translate`. |

Quote values that contain spaces inside the quoted `exec` command. Zathura's `exec` splits unquoted words into separate arguments.

## What gets captured

- The selection is read from zathura's clipboard. Zathura copies it when a mouse selection ends. `exec` has no `$SELECTION` placeholder. The adapter checks PRIMARY first, then CLIPBOARD, and prefers the text that appears in the extracted page. Unrelated clipboard text is rejected unless you force it with `--clipboard`. Hyphenated line breaks are joined for matching, so a selection containing `enhanc-\ning` still matches `enhancing` in the PDF text.
- The document is the current page plus `--context-pages` pages on each side, extracted with `pdftotext -f START -l END`. `pdftotext` clamps ranges beyond the last page, so the adapter does not need a page count.
- The file label is `<name> (page N)`, not the full path, so the provider does not see your home directory.
- Byte offsets are included only when the selection appears exactly in the extracted text. Otherwise the CLI falls back to the first exact occurrence. With the default one-page context window, the extracted text is usually under the CLI's 32k `auto` budget, so the whole window is sent even when the exact selection is not found.

## Answer viewers

The helper writes the request to a private temporary file, then launches the selected viewer in a terminal.

With the default `--viewer terminal`, the terminal runs `docent-zathura render REQUEST_FILE`. The render process sends the request to `docent run --profile NAME --stream --source zathura`, parses the NDJSON events, prints the answer, and waits for Enter before closing. Closing the terminal sends SIGHUP to the render process and cancels the request.

With `--viewer nvim`, the terminal runs Neovim with the request file. A small Lua viewer adds `plugins/nvim` to the runtimepath, loads the existing Neovim plugin, and calls `docent.run` with the explicit request data. The plugin streams the answer into its float with `filetype = "markdown"`. Press `q` to close the float; closing it while the request is still running cancels the request. The viewer preserves your existing plugin settings such as window size and keymaps, and only overrides the `docent` executable and the `source` value, which is recorded as `zathura` in run history.

With `--viewer qutebrowser`, the helper starts a small HTTP server bound to `127.0.0.1` with a random token in the URL. It launches `qutebrowser --target=TARGET URL`. The helper runs `docent run --profile NAME --stream --html --source zathura`; the page receives raw text deltas over Server-Sent Events and the final HTML rendered by the CLI when the stream finishes. Closing the page cancels the request. The default target is `window`; use `--qutebrowser-target tab` for a new tab in the existing qutebrowser window. The server is loopback-only and the URL is not reachable from other machines.

The page uses the same answer shell and `answer.css` as the qutebrowser adapter, so the visual style matches. It does not read qutebrowser's status-bar palette because it runs outside qutebrowser. Use `--qutebrowser-css PATH` to append your own CSS after the answer styles, for example:

```zathurarc
map <C-e> exec "/home/you/path/to/reading-assistant/plugins/zathura/docent-zathura explain $FILE $PAGE --viewer qutebrowser --qutebrowser-css /home/you/.config/docent/answer.css"
```

The terminal viewer and Neovim viewer use a separate terminal window, not a zathura overlay. On a tiling compositor, add a float rule for the `docent` app id (foot and kitty) or the `Docent` class (xterm). The adapter sets those names for the supported terminals. On an error, the terminal stays open so the message is readable.

## Troubleshooting

- `no text selected`: select text with the mouse first. The adapter checks both clipboards; if neither contains the selection, select again or pass `--primary` or `--clipboard`.
- `the CLIPBOARD selection does not appear on this page`: the clipboard holds unrelated text. Select the passage again, or force the target with `--clipboard` if the text extraction differs from the viewer.
- `pdftotext not found`: install poppler-utils.
- `no text layer`: the PDF is scanned or the page has no text. OCR and vision input are not implemented.
- `no terminal emulator found`: set `DOCENT_TERMINAL` or pass `--terminal`.
- `nvim not found`: install Neovim or pass `--nvim` with an absolute path.
- `could not load the docent Neovim plugin`: check that `plugins/nvim/` exists in this checkout and that Neovim is 0.10 or newer.
- `qutebrowser not found`: install qutebrowser or pass `--qutebrowser` with an absolute path.
- `qutebrowser did not open the answer page`: qutebrowser did not connect to the local server within the timeout. Check the `--qutebrowser-target` value and the qutebrowser process.
- `could not start docent`: use an absolute `--docent` path or fix `PATH` for the process that starts zathura.
- `missing API key`: export the variable named by `api_key_env` in the session that starts zathura.
- The key does nothing: `exec` is disabled in `zathura-sandbox`, and a binding may not apply in every zathura mode. Check `zathura --version` and the `map` mode.

## Limitations

- Zathura has no keyboard text selection; the workflow is mouse-first.
- The adapter is PDF only. DjVu, PostScript, and comic-book files need different extractors.
- The selection must stay in the clipboard until the key is pressed. If `selection-clipboard` is `false`, zathura does not copy the selection and the adapter cannot work.
- The terminal and Neovim viewers use a separate terminal window, and the qutebrowser viewer uses a separate browser page. An in-app zathura overlay would require patching zathura's C/GTK core, because the plugin API only covers document backends.
- `--viewer nvim` requires Neovim 0.10 or newer and the in-repository plugin. The answer appears in the plugin's float, not in a full editor buffer.
- `--viewer qutebrowser` requires qutebrowser 3.7 or newer. It streams raw text and shows the HTML rendered by the CLI when the stream finishes, matching the qutebrowser adapter's answer tab. The page is served from a loopback-only HTTP server for the duration of the request.
- `zathura-sandbox` disables `exec`.
- The adapter is not a zathura plugin and does not use zathura's plugin ABI.
