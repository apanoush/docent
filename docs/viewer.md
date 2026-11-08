# CLI browser viewer

The terminal commands can stream the answer into qutebrowser instead of stdout. The CLI starts a small HTTP server on `127.0.0.1`, opens a tokenized answer page, and sends the answer over Server-Sent Events. Closing the page cancels the request.

This is the same answer page and stylesheet used by the qutebrowser adapter, but it does not need qutebrowser's plugin API or status-bar palette.

## Requirements

- qutebrowser 3.7 or newer, available on `PATH` or passed with `--qutebrowser`.
- A graphical session with `DISPLAY` or `WAYLAND_DISPLAY` set.
- The CLI does not need Python or any adapter files at runtime.

## Usage

```bash
docent explain notes.md --lines 12:20 --viewer qutebrowser
docent ask notes.md --lines 12:20 --question "Why does this work?" --viewer qutebrowser
docent translate notes.md --line 12 --to French --viewer qutebrowser
docent summarize paper.md --viewer qutebrowser
```

The same flags work for `docent do` and `docent run`:

```bash
printf '%s' '{"selection":"transformer","document":"A transformer maps a sequence to a sequence."}' |
  docent run --profile explain --viewer qutebrowser --header notes.md
```

## Options

| Option | Default | Meaning |
|---|---|---|
| `--viewer {terminal,qutebrowser}` | `terminal` | Show the answer in the terminal or in qutebrowser. |
| `--qutebrowser PATH` | `qutebrowser` | Qutebrowser executable or command name. |
| `--qutebrowser-target TARGET` | `window` | `auto`, `tab`, `tab-bg`, `tab-silent`, `tab-bg-silent`, `window`, or `private-window`. |
| `--answer-css PATH` | none | Extra CSS appended after the built-in styles and `<config>/answer.css`. |
| `--header TEXT` | request file | `docent run` only; text shown after the profile in the page header. |

`--qutebrowser-target` is passed to qutebrowser as `--target=TARGET`. Qutebrowser applies that value only when an instance is already running; otherwise it opens a window. Use `--qutebrowser-target tab` to reuse a running qutebrowser window, or `window` for a separate answer window.

`--viewer qutebrowser` cannot be combined with `--no-stream`, `--stream`, `--json`, or `--print-prompt`. The answer is not written to stdout.

## Answer style

The page uses the default styles from `plugins/qutebrowser/docent/answer.css`, which are compiled into the CLI. CSS is applied in this order:

1. The built-in styles.
2. `<config dir>/answer.css`, when the file exists.
3. The file passed with `--answer-css`.

The file created by `docent init` is a commented example, so it changes nothing until you edit it. See [customizing the answer style](qutebrowser.md#customizing-the-answer-style) for the available custom properties.

## Rendering and protocol

While the answer streams, the page shows plain text. When the stream finishes, the CLI renders the answer as Markdown and sends one `html` event. The same renderer serves `docent run --stream --html`, which is how the qutebrowser, Firefox, and zathura adapters render answers.

Raw HTML in a model answer is escaped, images stay alt text, and only `http`, `https`, and `mailto` links become clickable. Links open in a new tab. Tables, headings, lists, code fences, blockquotes, emphasis, and strikethrough are rendered.

## Security

The server binds only to `127.0.0.1` on a random port. The page URL contains a random 128-bit token, and the server exposes only the page and its event stream. The token is not reachable from another machine and is discarded when the viewer exits.

## Troubleshooting

- `no graphical session`: start Docent from a session with `DISPLAY` or `WAYLAND_DISPLAY`, or use the terminal viewer.
- `could not start qutebrowser`: pass an absolute `--qutebrowser` path or fix `PATH`.
- `qutebrowser did not open the answer page`: check the `--qutebrowser-target` value and that qutebrowser starts. The CLI waits 30 seconds for the page to connect.
- `connection lost`: the CLI exited before the answer finished, usually because of a provider error. The error line on the page shows the detail when the process could report it.
- Closing the page stops the provider request. The run is recorded with status `cancelled` when history is enabled.

## Limitations

- The page is a separate qutebrowser tab or window, not an overlay parented to the current page.
- The CLI cannot read qutebrowser's status-bar colors, because the page is served outside qutebrowser. Use `answer.css` to set the palette.
- Qutebrowser must already support `--target`; that is qutebrowser 3.7 or newer.
