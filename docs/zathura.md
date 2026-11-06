# Zathura adapter

The zathura adapter reads the selected text from the system clipboard, extracts the current page and its neighbors with `pdftotext`, calls the `docent` CLI, and streams the answer into a terminal window. It is an external adapter, not a zathura plugin: zathura's plugin API only loads document backends, so the integration uses a `zathurarc` `exec` binding.

## Requirements

- zathura with `$FILE` and `$PAGE` expansion in `exec`. The adapter targets zathura 2026.10.4 and the poppler plugin.
- Python 3.10 or newer for the adapter script.
- `pdftotext` from poppler-utils. The adapter is PDF only.
- `docent` installed and available on `PATH`, or an absolute path passed through `--docent`.
- A clipboard tool: `wl-paste` on Wayland, or `xclip` or `xsel` on X11.
- A terminal emulator: `foot`, `kitty`, `alacritty`, or `xterm`, or a command in `DOCENT_TERMINAL` or `--terminal`.

## Install

Add bindings to `~/.config/zathura/zathurarc`:

```zathurarc
map <C-e> exec /home/you/path/to/reading-assistant/plugins/zathura/docent-zathura explain $FILE $PAGE
map <C-a> exec /home/you/path/to/reading-assistant/plugins/zathura/docent-zathura ask $FILE $PAGE
map <C-t> exec /home/you/path/to/reading-assistant/plugins/zathura/docent-zathura translate $FILE $PAGE
```

Use absolute paths. Zathura launched from a desktop entry may not inherit your shell `PATH` or the API-key environment variable. Export the key in the session that starts zathura, as the [qutebrowser guide](qutebrowser.md) describes.

Select text with the mouse, then press the key. Zathura has no keyboard text selection, so this is a mouse-first workflow. The answer opens in a terminal window and streams as it arrives.

If you set `selection-clipboard clipboard` in `zathurarc`, add `--clipboard` to each binding:

```zathurarc
map <C-e> exec /path/docent-zathura explain $FILE $PAGE --clipboard
```

## Commands

| Command | Action |
|---|---|
| `docent-zathura explain FILE PAGE` | Explain the selection. |
| `docent-zathura ask FILE PAGE` | Ask a question. The terminal prompts for the question if `--question` is omitted. |
| `docent-zathura translate FILE PAGE` | Translate the selection. |
| `docent-zathura do PROFILE FILE PAGE` | Run a custom profile. |

Options:

| Option | Default | Meaning |
|---|---|---|
| `--clipboard` | off | Read the CLIPBOARD selection instead of PRIMARY. |
| `--terminal NAME` | detected | Terminal emulator to use. |
| `--docent PATH` | `docent` | CLI executable. |
| `--context-pages N` | `1` | Pages of context before and after the current page. |
| `--context-mode MODE` | profile or config | `auto`, `full`, or `none`. |
| `--question TEXT` | none | Question for `ask` or `do`; `ask` prompts when omitted. |
| `--target-language LANG`, `--to LANG` | profile or config | Translation target for `translate`. |

Quote values that contain spaces. Zathura's `exec` splits unquoted words into separate arguments.

## What gets captured

- The selection is read from zathura's clipboard. Zathura copies it when a mouse selection ends. `exec` has no `$SELECTION` placeholder.
- The document is the current page plus `--context-pages` pages on each side, extracted with `pdftotext -f START -l END`. `pdftotext` clamps ranges beyond the last page, so the adapter does not need a page count.
- The file label is `<name> (page N)`, not the full path, so the provider does not see your home directory.
- Byte offsets are included only when the selection appears exactly in the extracted text. Otherwise the CLI falls back to the first exact occurrence. With the default one-page context window, the extracted text is usually under the CLI's 32k `auto` budget, so the whole window is sent even when the exact selection is not found.

## Answer window

The helper writes the request to a private temporary file, then launches a terminal running `docent-zathura render REQUEST_FILE`. The render process sends the request to `docent run --profile NAME --stream`, parses the NDJSON events, prints the answer, and waits for Enter before closing. Closing the terminal sends SIGHUP to the render process and cancels the request.

The terminal is a separate window, not a zathura overlay. On a tiling compositor, add a float rule for the `docent` app id (foot and kitty) or the `Docent` class (xterm). The adapter sets those names for the supported terminals. On an error, the window stays open so the message is readable.

## Troubleshooting

- `no text selected`: select text with the mouse first. If zathura uses the regular clipboard, pass `--clipboard`.
- `pdftotext not found`: install poppler-utils.
- `no text layer`: the PDF is scanned or the page has no text. OCR and vision input are not implemented.
- `no terminal emulator found`: set `DOCENT_TERMINAL` or pass `--terminal`.
- `could not start docent`: use an absolute `--docent` path or fix `PATH` for the process that starts zathura.
- `missing API key`: export the variable named by `api_key_env` in the session that starts zathura.
- The key does nothing: `exec` is disabled in `zathura-sandbox`, and a binding may not apply in every zathura mode. Check `zathura --version` and the `map` mode.

## Limitations

- Zathura has no keyboard text selection; the workflow is mouse-first.
- The adapter is PDF only. DjVu, PostScript, and comic-book files need different extractors.
- The selection must stay in the clipboard until the key is pressed. If `selection-clipboard` is `false`, zathura does not copy the selection and the adapter cannot work.
- The answer is a separate terminal window. An in-app overlay would require patching zathura's C/GTK core, because the plugin API only covers document backends.
- `zathura-sandbox` disables `exec`.
- The adapter is not a zathura plugin and does not use zathura's plugin ABI.
