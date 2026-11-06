# Zathura adapter modules

The adapter lives in `plugins/zathura/`, separate from the Rust crate. It has no third-party dependencies and uses only the Python standard library. The `docent-zathura` script is the entry point; `docent_zathura/` is the package.

## File map

| File | Responsibility |
|---|---|
| `docent-zathura` | Executable entry point. Adds the package directory to `sys.path` and calls `cli.main`. |
| `docent_zathura/clipboard.py` | Selects and runs `wl-paste`, `xclip`, or `xsel`; returns the selected text. |
| `docent_zathura/pdf.py` | Page window calculation and `pdftotext` invocation. |
| `docent_zathura/request.py` | Builds the JSON request and optional UTF-8 byte offsets. |
| `docent_zathura/render.py` | Parses NDJSON, prints deltas, handles errors, and keeps the terminal open. |
| `docent_zathura/launch.py` | Detects a terminal emulator and builds its argv. |
| `docent_zathura/cli.py` | Argument parsing, capture orchestration, request-file creation, and error reporting. |

## Process model

The adapter uses two processes:

1. The capture process is started by zathura's `exec`. It reads the clipboard, extracts the page window, writes a private request file, and launches a terminal. It exits immediately.
2. The render process runs in the terminal. It reads and deletes the request file, starts `docent run --profile NAME --stream --source zathura`, writes the request JSON to stdin, and prints `delta` text as it arrives. It prints `error` events to stderr and waits for Enter after a successful answer.

The request file is created with `mkdtemp` under `XDG_RUNTIME_DIR` (or the system temporary directory), written with mode 0600, and removed by the render process. This keeps the selection and document text out of argv and out of the process list.

The capture process uses the JSON stdin contract rather than `docent explain --text`. That gives the request a proper `file` label and keeps the selection out of the command line. It also means the adapter uses the same streaming contract as the Neovim and qutebrowser integrations.

## Capture

`clipboard.py` prefers `wl-paste` when `WAYLAND_DISPLAY` or `XDG_SESSION_TYPE=wayland` is set, then `xclip` and `xsel` when `DISPLAY` is set. With no explicit target it reads PRIMARY first, then CLIPBOARD, and prefers the text that occurs in the extracted page. `--primary` and `--clipboard` force a target. The page check avoids sending unrelated clipboard text when the configured clipboard is empty or stale.

`pdf.py` computes `(max(1, page - radius), page + radius)` and runs `pdftotext -f START -l END FILE -`. It rejects empty output, which usually means the page is scanned.

`request.py` normalizes CRLF, strips a trailing newline from the selection, and adds `byte_start` and `byte_end` only when the selection occurs exactly in the UTF-8 document. The CLI revalidates the offsets and falls back to the first exact occurrence.

`cli.py` resolves `--docent` with `shutil.which`, then searches `~/.local/bin`, `~/.cargo/bin`, `/usr/local/bin`, and `/usr/bin`. This keeps desktop-launched zathura working when its `PATH` does not include the user's shell path.

## Rendering

`render.py` reads the request file and starts `docent run --profile NAME --stream --source zathura` with optional `--target-language` and `--context-mode`. It parses one JSON object per line:

- `delta`: write `text` to stdout and flush.
- `done`: stop reading and finish successfully.
- `error`: print `message` to stderr and exit 1.

A separate thread drains stderr so a chatty CLI cannot block on a full pipe. The render process prints a header with the profile and file label, then the answer. After success it prints `Press Enter to close.` and reads one line when stdin is a TTY. Closing the terminal sends SIGHUP to the process, which terminates the child.

## Terminal launch

`launch.py` supports `foot`, `kitty`, `alacritty`, and `xterm`, and falls back to `TERMINAL -e COMMAND` for other terminals. `DOCENT_TERMINAL` takes precedence over `TERMINAL`, and `--terminal` takes precedence over both. The supported terminals get an app id or class of `docent` or `Docent` so a compositor or window manager can float the answer window.

## Tests

`tests/zathura/run.sh` runs pytest, ruff, and ty. The unit tests cover clipboard backend selection, automatic PRIMARY and CLIPBOARD matching, page windows and `pdftotext` arguments, request offsets, terminal argv building, NDJSON parsing, error paths, request-file cleanup, `docent` lookup, error dialogs, and CLI orchestration.

`tests/zathura/e2e.sh` is a manual end-to-end check. It creates a small PDF with `ps2pdf`, puts a selection on the clipboard, runs the adapter with a fake terminal and a fake `docent`, and checks the streamed answer. It needs a display and a clipboard tool and is not part of `scripts/check.sh`.

## Design notes

- The adapter is external because zathura's plugin API only loads document backends. A true in-app overlay would need a zathura core patch.
- The answer is a terminal window because it streams without a GUI toolkit and matches the CLI's terminal output. A `zenity` dialog was rejected for the first version because it cannot stream.
- The context is a page window, not the whole PDF, to keep the request small and relevant. The CLI's `auto` mode sends the whole window when it fits.
- The selection is read from the clipboard because `exec` has no `$SELECTION` placeholder. A future zathura change could pass the selection directly, but the clipboard path works with current releases.
