# CLI browser viewer

The terminal commands can stream the answer into qutebrowser or Firefox instead of stdout. The CLI starts a small HTTP server on `127.0.0.1`, opens a tokenized answer page, and sends the answer over Server-Sent Events. Closing the page cancels the request.

This is the same answer page and stylesheet used by the qutebrowser and Firefox adapters, but it does not need qutebrowser's plugin API, the Firefox extension, or a browser status-bar palette.

## Requirements

- qutebrowser 3.7 or newer for `--viewer qutebrowser`, available on `PATH` or passed with `--qutebrowser`.
- Firefox with the standard `--new-tab`, `--new-window`, and `--private-window` URL flags for `--viewer firefox`, available on `PATH` or passed with `--firefox`.
- On X11 and Wayland systems, a graphical session with `DISPLAY` or `WAYLAND_DISPLAY` set. macOS and Windows need no environment variable.
- The CLI does not need Python or any adapter files at runtime.

## Usage

```bash
docent explain notes.md --lines 12:20 --viewer qutebrowser
docent explain notes.md --lines 12:20 --viewer firefox
docent ask notes.md --lines 12:20 --question "Why does this work?" --viewer firefox
docent translate notes.md --line 12 --to French --viewer firefox
docent summarize paper.md --viewer firefox --firefox-target tab
```

The same flags work for `docent do` and `docent request`:

```bash
printf '%s' '{"selection":"transformer","document":"A transformer maps a sequence to a sequence."}' |
  docent request --profile explain --viewer firefox --header notes.md
```

## Default viewer

Set the viewer once in `config.toml` instead of passing `--viewer` on every command:

```toml
[viewer]
default = "firefox"          # terminal | qutebrowser | firefox
qutebrowser = "qutebrowser"
qutebrowser_target = "window"
# qutebrowser_args = ["--qt-arg", "name", "qute-docent"]
firefox = "firefox"
firefox_target = "window"
```

A partial table is valid, so the `default` key alone is enough. Each key falls back to its built-in default when omitted. CLI flags win over the config for one run, for example `--viewer terminal`.

The configured default applies to `explain`, `ask`, `translate`, `summarize`, and `do`. `docent request` keeps `terminal` as its default because the Neovim, qutebrowser, Firefox, and zathura adapters call it with `--stream`; pass `--viewer` explicitly to open a browser page from `docent request`. The configured executable and target still apply when you do.

## Options

| Option | Default | Meaning |
|---|---|---|
| `--viewer {terminal,qutebrowser,firefox}` | `viewer.default`, else `terminal` | Show the answer in the terminal, qutebrowser, or Firefox. |
| `--qutebrowser PATH` | `viewer.qutebrowser`, else `qutebrowser` | Qutebrowser executable or command name. |
| `--qutebrowser-target TARGET` | `viewer.qutebrowser_target`, else `window` | `auto`, `tab`, `tab-bg`, `tab-silent`, `tab-bg-silent`, `window`, or `private-window`. |
| `--qutebrowser-arg ARG` | `viewer.qutebrowser_args`, else none | Extra qutebrowser argument inserted before the answer URL. Repeat the flag to pass several arguments. |
| `--firefox PATH` | `viewer.firefox`, else `firefox` | Firefox executable or command name. |
| `--firefox-target TARGET` | `viewer.firefox_target`, else `window` | `auto`, `tab`, `window`, or `private-window`. |
| `--answer-css PATH` | none | Extra CSS appended after the built-in styles and `<config>/answer.css`. `--qutebrowser-css` is an alias. |
| `--header TEXT` | request file | `docent request` only; text shown after the profile in the page header. |

`--qutebrowser-target` is passed to qutebrowser as `--target=TARGET`. Qutebrowser applies that value only when an instance is already running; otherwise it opens a window. Use `--qutebrowser-target tab` to reuse a running qutebrowser window, or `window` for a separate answer window. The `--qutebrowser`, `--qutebrowser-target`, and `--qutebrowser-arg` options apply only to `--viewer qutebrowser`.

`qutebrowser_args` and repeated `--qutebrowser-arg` flags are appended to the qutebrowser command line after `--target` and before the answer URL. Each list entry or flag becomes one argv entry, so arguments with spaces stay intact and are never interpreted by a shell. When `--qutebrowser-arg` is passed, it replaces `viewer.qutebrowser_args` for that run. Some qutebrowser arguments only take effect when qutebrowser starts a new instance or window rather than reusing a running one.

`--firefox-target` maps to Firefox's URL flags: `tab` passes `--new-tab`, `window` passes `--new-window`, `private-window` passes `--private-window`, and `auto` passes no flag. Firefox applies the target to a running instance when one exists. The `--firefox` and `--firefox-target` options apply only to `--viewer firefox`.

`--viewer firefox` opens a standalone local answer page. It does not use the Docent Firefox extension; the extension remains the way to capture a page selection and run a profile from inside Firefox.

Neither browser viewer can be combined with `--no-stream`, `--stream`, `--json`, `--html`, or `--print-prompt`. The answer is not written to stdout.

## Answer style

The page uses the default styles from `plugins/qutebrowser/docent_qutebrowser/answer.css`, which are compiled into the CLI. CSS is applied in this order:

1. The built-in styles.
2. `<config dir>/answer.css`, when the file exists.
3. The file passed with `--answer-css`.

The file created by `docent init` is a commented example, so it changes nothing until you edit it. See [customizing the answer style](qutebrowser.md#customizing-the-answer-style) for the available custom properties.

## Rendering and protocol

While the answer streams, the page shows plain text. When the stream finishes, the CLI renders the answer as Markdown and sends one `html` event. The same renderer serves `docent request --stream --html`, which is how the qutebrowser, Firefox, and zathura adapters render answers.

Raw HTML in a model answer is escaped, images stay alt text, and only `http`, `https`, and `mailto` links become clickable. Links open in a new tab. Tables, headings, lists, code fences, blockquotes, emphasis, and strikethrough are rendered.

TeX math is rendered as MathML, so it works without JavaScript or a network connection. Inline `\(...\)` and `$...$` and display `\[...\]` and `$$...$$` are recognized. Code spans, code blocks, and link destinations keep their original TeX. A formula that the MathML renderer cannot handle stays visible as TeX instead of breaking the answer.

## Security

The server binds only to `127.0.0.1` on a random port. The page URL contains a random 128-bit token, and the server exposes only the page and its event stream. The token is not reachable from another machine and is discarded when the viewer exits. Answer pages load no scripts or remote resources; math is plain MathML in the HTML fragment.

## Troubleshooting

- `no graphical session`: start Docent from an X11 or Wayland session with `DISPLAY` or `WAYLAND_DISPLAY`, or use the terminal viewer. macOS and Windows do not report this error.
- `could not start qutebrowser`: pass an absolute `--qutebrowser` path or fix `PATH`.
- `could not start Firefox`: pass an absolute `--firefox` path or fix `PATH`. Snap and Flatpak installs may need a wrapper script.
- `qutebrowser did not open the answer page`: check the `--qutebrowser-target` value and that qutebrowser starts. The CLI waits 30 seconds for the page to connect.
- `Firefox did not open the answer page`: check the `--firefox-target` value and that Firefox starts. The CLI waits 30 seconds for the page to connect.
- `connection lost`: the CLI exited before the answer finished, usually because of a provider error. The error line on the page shows the detail when the process could report it.
- Closing the page stops the provider request. The run is recorded with status `cancelled` when history is enabled.

## Limitations

- The page is a separate browser tab or window, not an overlay parented to the current page.
- The CLI cannot read qutebrowser's status-bar colors, because the page is served outside qutebrowser. Use `answer.css` to set the palette.
- Qutebrowser must already support `--target`; that is qutebrowser 3.7 or newer.
- `--viewer firefox` does not use the Firefox extension, so extension keybindings are not available on the answer page.
- Firefox target flags apply to a running instance. Choosing a Firefox profile is out of scope.
- `--firefox` takes one executable or path. Flatpak and Snap installs can use a wrapper script.
