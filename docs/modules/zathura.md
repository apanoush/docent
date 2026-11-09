# Zathura adapter modules

The adapter lives in `plugins/zathura/`, separate from the Rust crate. It has no third-party dependencies and uses only the Python standard library. The `docent-zathura` script is the entry point; `docent_zathura/` is the package.

## File map

| File | Responsibility |
|---|---|
| `docent-zathura` | Executable entry point. Adds the package directory to `sys.path` and calls `cli.main`. |
| `docent_zathura/clipboard.py` | Selects and runs `wl-paste`, `xclip`, or `xsel`; returns the selected text. |
| `docent_zathura/pdf.py` | Page window calculation and `docent extract` invocation. |
| `docent_zathura/request.py` | Builds the JSON request, document-only requests for summarize, and optional UTF-8 byte offsets. |
| `docent_zathura/render.py` | Parses NDJSON, prints deltas, handles errors, and keeps the terminal open. |
| `docent_zathura/logs.py` | Extracts `[WARN ...]` lines from CLI stderr, without the log prefix. |
| `docent_zathura/viewer.lua` | Neovim viewer. Reads the request file, loads the plugin, and calls `docent.run`. |
| `docent_zathura/qutebrowser_view.py` | qutebrowser viewer. Loopback HTTP server, SSE stream, shared answer shell, and final Markdown rendering. |
| `docent_zathura/launch.py` | Detects a terminal emulator and builds its argv. |
| `docent_zathura/cli.py` | Argument parsing, capture orchestration, viewer selection, request-file creation, and error reporting. |

## Process model

The adapter uses two processes:

1. The capture process is started by zathura's `exec`. It reads the clipboard, extracts the page window, writes a private request file, and launches a terminal. It exits immediately.
2. The viewer process runs in the terminal. With the default terminal viewer it starts `docent request --profile NAME --stream --source zathura`, writes the request JSON to stdin, and prints `delta` text as it arrives. With `--viewer nvim`, Neovim reads and deletes the request file, loads `plugins/nvim`, and calls `docent.run` with the explicit request data.

The qutebrowser viewer skips the terminal and request file. The capture process starts a loopback HTTP server, launches qutebrowser with a tokenized URL, and runs `docent request --profile NAME --stream --html --source zathura` when the page opens the SSE connection. The request stays in memory and is written to the CLI's stdin.

For the terminal and Neovim viewers, the request file is created with `mkdtemp` under `XDG_RUNTIME_DIR` (or the system temporary directory), written with mode 0600, and removed by the viewer. This keeps the selection and document text out of argv and out of the process list.

The capture process uses the JSON stdin contract rather than `docent explain --text`. That gives the request a proper `file` label and keeps the selection out of the command line. It also means the adapter uses the same streaming contract as the Neovim and qutebrowser integrations.

## Capture

`clipboard.py` prefers `wl-paste` when `WAYLAND_DISPLAY` or `XDG_SESSION_TYPE=wayland` is set, then `xclip` and `xsel` when `DISPLAY` is set. With no explicit target it reads both clipboards and prefers the longest text that occurs in the extracted page, using an exact match before a whitespace-normalized one. That stops a short stale token from shadowing the selection. When neither occurs in the page, it uses the longer text. `--primary` and `--clipboard` force a target. The page check avoids sending unrelated clipboard text when the configured clipboard is empty or stale. Matching joins hyphenated line breaks, so `enhanc-\ning` matches `enhancing` in the extracted output.

`pdf.py` computes `(max(1, page - radius), page + radius)` and runs `docent extract FILE --pages START:END`. `docent extract` uses the same PDF reader as the terminal commands and clamps the end page, so a window around the last page does not need a page count. It rejects empty output, which usually means the page is scanned.

`request.py` normalizes CRLF, strips a trailing newline from the selection, and adds `byte_start` and `byte_end` only when the selection occurs exactly in the UTF-8 document. The CLI revalidates the offsets and falls back to its whitespace-tolerant search.

`cli.py` resolves `--docent` with `shutil.which`, then searches `~/.local/bin`, `~/.cargo/bin`, `/usr/local/bin`, and `/usr/bin`. This keeps desktop-launched zathura working when its `PATH` does not include the user's shell path.

## Rendering

`render.py` reads the request file and starts `docent request --profile NAME --stream --source zathura` with optional `--target-language` and `--context-mode`. It parses one JSON object per line:

- `delta`: write `text` to stdout and flush.
- `done`: stop reading and finish successfully.
- `error`: print `message` to stderr and exit 1.

A separate thread drains stderr so a chatty CLI cannot block on a full pipe. The render process prints a header with the profile and file label, then the answer. After success it prints `Press Enter to close.` and reads one line when stdin is a TTY. Closing the terminal sends SIGHUP to the process, which terminates the child.

`qutebrowser_view.py` uses `ThreadingHTTPServer` on `127.0.0.1:0` and a random token path. The page shell comes from the qutebrowser adapter's `docent_qutebrowser.ui.build_shell` with a custom label, no `d close` hint, and an SSE script appended before `</body>`. The CSS is the same `answer.css`; `--qutebrowser-css` appends a user file after it. When the profile needs a question, the page shows a question form before the server starts the request. The server starts `docent request --profile NAME --stream --html --source zathura` only after the page connects and, when needed, the question is submitted, sends one JSON object per SSE `data:` line, and forwards the CLI's `html` event when the stream finishes. The page replaces the raw `<pre>` with the rendered HTML. A closed page sets a cancellation flag, terminates `docent`, and shuts down the server. The server never binds to a public interface.

## Terminal launch

`launch.py` supports `foot`, `kitty`, `alacritty`, and `xterm`, and falls back to `TERMINAL -e COMMAND` for other terminals. `DOCENT_TERMINAL` takes precedence over `TERMINAL`, and `--terminal` takes precedence over both. The supported terminals get an app id or class of `docent` or `Docent` so a compositor or window manager can float the answer window.

The Neovim viewer is launched through the same terminal layer. The capture process passes three environment variables to the terminal:

- `DOCENT_ZATHURA_REQUEST`: the private request file.
- `DOCENT_ZATHURA_VIEWER`: the absolute path to `viewer.lua`.
- `DOCENT_ZATHURA_NVIM`: the absolute path to `plugins/nvim`.

Neovim adds the plugin path to the runtimepath, runs the viewer script, and the script deletes the request file after reading it. The script merges the resolved `docent` path into the plugin's current configuration, so user settings such as window size, border, and keymaps are preserved.

## Tests

`tests/zathura/run.sh` runs pytest, ruff, and ty. The unit tests cover clipboard backend selection, automatic PRIMARY and CLIPBOARD matching, page windows and `docent extract` arguments, request offsets, terminal argv building, NDJSON parsing, error paths, request-file cleanup, `docent` lookup, error dialogs, and CLI orchestration.

`tests/zathura/e2e.sh` is a manual end-to-end check. It creates a small PDF with `ps2pdf`, puts a selection on the clipboard, runs the adapter with a fake terminal and a fake `docent`, and checks the streamed answer. It needs a display and a clipboard tool and is not part of `scripts/check.sh`.

`tests/zathura/e2e_nvim.sh` is a headless end-to-end check for the Neovim viewer. It runs the viewer script in `nvim --headless` with a fake `docent`, waits for the float, checks the streamed text, and verifies that the request file is removed. `tests/zathura/run.sh` runs it when Neovim is available.

`tests/zathura/test_qutebrowser_view.py` checks the qutebrowser viewer without a browser. It starts the server with a fake `docent` and a fake qutebrowser HTTP client, then verifies the final rendered Markdown, the error event, the missing-qutebrowser path, and the connection timeout.

## Design notes

- The adapter is external because zathura's plugin API only loads document backends. A true in-app overlay would need a zathura core patch.
- The answer is a terminal window because it streams without a GUI toolkit and matches the CLI's terminal output. A `zenity` dialog was rejected for the first version because it cannot stream.
- The Neovim viewer reuses the existing plugin instead of duplicating the float, stream parser, and Markdown buffer setup. The plugin already sets `filetype = "markdown"` and handles cancellation when the float closes.
- The qutebrowser viewer uses a URL and a loopback server rather than a qutebrowser command because qutebrowser's `--target` applies to URLs. Commands use `new_instance_open_target` instead. A normal URL is the reliable way to honor `--target=window` and the other target values.
- The qutebrowser viewer reuses `docent_qutebrowser.ui.build_shell` and `answer.css` from the qutebrowser adapter, and consumes the CLI's `html` event for the final rendering. It streams raw text during the request and shows the final HTML at the end, matching the qutebrowser answer tab. The only visual difference is that the standalone page has no qutebrowser status-bar palette; `--qutebrowser-css` covers user overrides.
- The context is a page window, not the whole PDF, to keep the request small and relevant. The CLI's `auto` mode sends the whole window when it fits.
- The selection is read from the clipboard because `exec` has no `$SELECTION` placeholder. A future zathura change could pass the selection directly, but the clipboard path works with current releases.
