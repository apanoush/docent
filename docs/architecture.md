# Architecture

Docent is a one-shot CLI with a terminal front end, an editor adapter, browser adapters, and a document-viewer adapter. Neovim owns the buffer and the editor UI; qutebrowser and Firefox own the page and the browser UI; zathura owns the PDF view; the CLI owns configuration, input resolution, prompting, context assembly, and provider communication.

```text
Terminal input          Neovim buffer        qutebrowser page      Firefox page
FILE, stdin, --lines,   selection, document, selection, innerText, selection, innerText,
--phrase, --text        byte offsets, file   page URL and title   page URL and title
        |                     |                    |                     |
        v                     v                    v                     v
input adapter          JSON on stdin        JSON on stdin        native host -> JSON
and prompts                   |                    |                     |
        |                     |                    |                     |
        +---------------------+--------------------+---------------------+
                                            |
                                            v
                                     docent executor
                           |-- load config.toml
                           |-- load profiles/<name>.toml
                           |-- resolve effective settings
                           |-- build context window
                           |-- render prompt template
                           |-- reuse or store a cached answer
                           |-- call provider
                                  |
                                  v
                    plain text, JSON, NDJSON events, or browser SSE
                                  |
              +-------------------+-------------------+
              |                   |                   |
              v                   v                   v
       terminal output     Neovim float      browser answer tabs
```

## Request contract

The CLI reads one JSON object from stdin:

```json
{
  "selection": "the selected passage",
  "document": "full buffer text",
  "byte_start": 1234,
  "byte_end": 1350,
  "file": "/path/to/notes.md",
  "question": null
}
```

Every field is optional. Unknown fields are rejected. The document can be large, so stdin is preferable to command-line arguments or environment variables. Neovim's `vim.system` accepts a string for stdin, which removes the need for temporary files. See `:h vim.system()` in Neovim 0.10 or newer. The qutebrowser adapter uses `QProcess` and writes the same JSON to its stdin.

The Neovim plugin sends UTF-8 byte offsets because that is what the buffer API exposes. The CLI trusts the offsets only when they match the selection exactly. Otherwise it falls back to the first exact occurrence. This keeps markers correct when a buffer was edited between capture and request. The first qutebrowser version omits offsets and relies on the same fallback.

## Terminal input adapter

The terminal commands (`explain`, `ask`, `translate`, `summarize`, `do`) build the same `Request` as the Neovim plugin. The `input` module handles the difference in input shapes:

- A file is read as the document. `-` means stdin.
- With no file and piped stdin, the piped text becomes the document.
- With no file and a terminal stdin, Docent prompts for a file.
- Line ranges are 1-based and inclusive and are converted to byte offsets through a line index.
- `--phrase` uses the first exact phrase occurrence and fails when the phrase is absent.
- `--text` supplies a selection without line numbers. Offsets are resolved when the text occurs in the document.
- CRLF is normalized to LF before the line index is built, so offsets and selections stay consistent.

Prompts use a small `Prompter` trait. The real implementation reads from stdin and writes labels to stderr; tests substitute a scripted implementation. Prompts only run when stdin is a terminal, so a piped document is never consumed as prompt input. The profile is loaded before input resolution, which lets its `requires` list drive file, selection, and question prompts.

## qutebrowser adapter

The adapter is a Python module loaded from qutebrowser's `config.py`. It captures the page selection with `window.getSelection()` and the rendered page text with `document.body.innerText` through `tab.run_js_async`. The page URL has its query and fragment removed before it is used as the `file` label. A profile that declares `requires = ["question"]` opens a `QInputDialog` before the request starts.

The adapter starts `docent request --profile NAME --stream --html --source qutebrowser` with `QProcess`, writes the request JSON to stdin, and parses stdout with the same NDJSON line parser as the Neovim plugin. The answer streams into a new tab, and the CLI's final `html` event replaces the raw text with the rendered Markdown. Closing the tab calls `terminate()` and then `kill()` if needed. The question travels in the JSON request rather than in argv so it does not appear in the process list.

qutebrowser does not expose a stable third-party plugin API. The loader only walks built-in components, and `qutebrowser/api/__init__.py` says the extension API is not exposed to third-party extensions yet. The adapter therefore imports internal modules and must be retested on qutebrowser upgrades. It also requires JavaScript for selection capture and answer rendering. `window.getSelection()` sees the top frame only, so iframes, shadow DOM, and PDF viewers are not supported in the first version.

The first UI is a tab rather than a floating window. qutebrowser's application-wide key filter handles keys before widgets, and `q` is bound to macro recording in normal mode, so a float-like overlay would need insert/passthrough mode or a plugin key filter. A tab keeps normal qutebrowser bindings, including `d` to close and `j`/`k` to scroll.

## Firefox adapter

Firefox extensions cannot start processes, so the adapter has two parts. `plugins/firefox/` is a Manifest V2 WebExtension, and `docent-native-host` is a small Rust binary that Firefox starts through native messaging. The extension captures the selection and `document.body.innerText` with `tabs.executeScript` after an `activeTab` grant, builds the same JSON request as the other adapters, and sends it through the host. The host starts `docent request --profile NAME --stream --html --source firefox`, forwards each stdout line as one native message, and reports the exit code with the captured stderr.

Closing the answer tab disconnects the native port. Firefox sends `SIGTERM` to the host and its child, then `SIGKILL` after a grace period, so the request is cancelled without host-side signal handling. One host process handles one run. The profile catalog and the user stylesheet use `runtime.sendNativeMessage`, which starts a short-lived host per call.

The answer page is an extension page with its own CSP, so it does not inject scripts into the source page. It shows the HTML from the CLI's `html` event and applies `<config dir>/answer.css` when the host can read it.

The extension requests `activeTab` rather than host permissions, so it cannot read pages until the user clicks the toolbar button, uses the shortcut, or picks a context-menu item. Firefox extension shortcuts are modifier combinations, not key chains, so a toolbar popup and a context menu replace qutebrowser's `,de` bindings. Offsets are omitted for the same reason as qutebrowser. Reader view, the PDF viewer, view-source, and browser UI pages cannot be scripted.

## Zathura adapter

Zathura has no third-party UI plugin API. Its plugin system loads document backends by MIME type, and the only scripting hooks are `zathurarc` commands, `exec`, and D-Bus. The adapter therefore uses a `zathurarc` binding:

```zathurarc
map <C-e> exec "/path/to/docent-zathura explain $FILE $PAGE"
```

`exec` expands `$FILE` and `$PAGE` and starts the helper directly, without a shell. The helper cannot receive the selection directly because `exec` has no `$SELECTION` placeholder. Zathura copies a mouse selection to the clipboard, so the helper reads it with `wl-paste`, `xclip`, or `xsel`. It checks both PRIMARY and CLIPBOARD and prefers the longest text that occurs in the extracted page, which keeps the default binding working with either `selection-clipboard` setting and stops a short stale token from winning. The workflow is mouse-first because zathura has no keyboard text selection.

The helper extracts the current page plus one page on each side with `pdftotext`, builds the same JSON request as the other adapters, and writes it to a private temporary file. It resolves `docent` through `PATH` and then through common install locations such as `~/.local/bin` and `~/.cargo/bin`, because desktop-launched zathura often has a minimal `PATH`. It then starts a terminal window running `docent-zathura render REQUEST_FILE`. The render process reads and deletes the request file, runs `docent request --profile NAME --stream --source zathura`, prints the `delta` events, and waits for Enter before closing. This two-process split keeps the selection out of argv and gives the answer a window that stays open after the stream ends.

The answer is a terminal window rather than a zathura overlay. An overlay would require patching zathura's GTK/C core. The terminal window streams naturally, supports the CLI's existing output, and can be floated with a compositor rule. With `--viewer nvim`, the same request file is handed to the existing Neovim plugin, which streams into its Markdown float. That reuses the plugin's stream parser, cancellation, and buffer setup instead of duplicating them in the zathura adapter. With `--viewer qutebrowser`, the adapter starts a loopback HTTP server, opens a tokenized URL with `qutebrowser --target=TARGET`, streams raw text over Server-Sent Events, and forwards the CLI's final `html` event. The page also reuses the qutebrowser adapter's answer shell and `answer.css`, so the visual style matches the answer tab. The URL approach is required because qutebrowser's `--target` applies to URLs, not commands. The first version is PDF only; DjVu, PostScript, and comic-book files need different text extractors.

## CLI browser viewer

The terminal commands and `docent request` can open a loopback answer page in qutebrowser or Firefox with `--viewer`. The server, page shell, CSS cascade, Markdown event, cancellation, and history handling are shared. Only the launch arguments differ: qutebrowser uses `--target=VALUE`, and Firefox uses `--new-tab`, `--new-window`, `--private-window`, or no flag for `auto`. The terminal commands read their default viewer, executable, and target from `[viewer]` in `config.toml`; `docent request` keeps the terminal default unless `--viewer` is passed, so adapters that call it with `--stream` are unaffected. `Viewer::open` waits up to 30 seconds for the page's event stream, then fails with a browser-specific message. Closing the page sets the cancellation flag and the provider request is aborted. The Firefox target does not use the Firefox extension; the extension remains a selection-capture client.

## Output modes

Terminal commands stream plain text to stdout by default and flush after each fragment. `--no-stream` waits for the complete answer and prints it once. `--viewer qutebrowser` or `--viewer firefox` streams the answer into a loopback Server-Sent Events page instead of stdout and renders Markdown once at the end. The Neovim, qutebrowser, Firefox, and zathura integrations use `docent request --stream`, which emits newline-delimited JSON instead. `docent request --json` wraps the final answer in one JSON object, and `--stream --html` adds a rendered HTML event for browser front ends.

The shared executor builds the context and prompt once, then selects the output mode or replays a cached answer. This keeps buffered, streamed, JSON, and prompt-preview paths on the same request and prompt code.

## Run history

When history is enabled, `cli::execute` records one row after the provider call returns or fails. The row stores the command, profile, source, structured request inputs, effective settings, the rendered system and user messages, the answer, token usage, duration, status, and error. Streamed runs record the partial answer when the stream fails. `--print-prompt` and failures before the provider call are not recorded.

`storage` opens the SQLite database once per run, enables WAL mode, applies a busy timeout, and creates the schema on first use. Schema changes are tracked with `PRAGMA user_version`. The database path comes from `history.database` or the platform data directory. A failed write is logged as a warning and never changes the command result.

The global `--source` flag, default `cli`, identifies the caller. The adapters pass `nvim`, `qutebrowser`, `firefox`, or `zathura`; custom plugins pass their own name. The source is only stored in history and does not affect request handling.

The database contains full prompts and answers, including document context, so it is local, unencrypted, and restricted to the owning user on Unix. API keys are not stored. See [run history](history.md) for the schema and queries.

## Answer cache

The executor checks the answer cache after `prompt::render` and before the provider call. The key is a BLAKE3 hash over a version tag, `provider.wire_format`, `provider.base_url`, the model, the temperature bits, `max_tokens`, and the rendered system and user messages. The rendered prompt already contains the selection, context window, question, languages, and file label, so those inputs need no separate hashing. Byte offsets, command, profile, source, and output mode are excluded because they do not change the answer.

A hit synthesizes a `Completion` with `cached: true` and no token usage, then delivers its text through the same `on_delta` callback as a provider stream. Every output mode therefore behaves the same way: plain streaming prints one fragment, NDJSON emits one `delta`, the `html` event is rendered locally from the stored Markdown, and the browser viewer streams it normally. A `done` event gains `"cached": true`; the adapters ignore the extra field.

A successful provider answer is stored unless the run used `--no-cache` or the cache is disabled. Failures, cancelled runs, and empty outputs are never stored. A hit is not written to `runs`, because it is not a provider call; the cache row increments `hits` and updates `last_used_at` instead. Entries are evicted least recently used beyond `cache.max_entries`.

The cache lives in the same SQLite database as run history and shares its schema version. It is active only when `history.enabled` is true, the run did not pass `--no-history`, and `cache.enabled` is true. `--no-cache` skips reads and writes; `--refresh` skips reads but replaces the stored answer. Like history, cache reads and writes are fail-open and never change the command result.

A hit does not require an API key, so previously seen content is available offline. At a nonzero temperature the first stored sample wins; `--refresh` is the reroll path. A silent provider model update behind the same model name is masked until the entry is refreshed.

## Settings and precedence

Settings resolve in this order, highest first:

1. CLI flags (`--context-mode`, `--target-language`).
2. Profile fields.
3. `config.toml`.
4. Built-in defaults.

Provider selection, base URL, API key environment variable, and timeout are global. Model, temperature, token limit, context mode, and languages can be overridden per profile. Keeping provider selection global avoids mixing credentials and wire formats in a single request.

Viewer selection is separate from prompt settings. The `--viewer`, `--qutebrowser`, `--qutebrowser-target`, `--firefox`, and `--firefox-target` flags override the `[viewer]` table, which overrides the built-in terminal default. Profiles do not carry viewer settings. `docent request` ignores the configured viewer default and keeps the terminal unless `--viewer` is passed, so adapters that call `--stream` are never switched to a browser.

## Context assembly

`context.mode` controls how much document text is sent:

- `auto`: send the whole document when it fits within `max_chars`, otherwise send a line-aligned window around the selection.
- `full`: send the whole document, regardless of `max_chars`.
- `none`: send no document.

The selection is wrapped in `[[[ SELECTION START ]]]` and `[[[ SELECTION END ]]]` inside the context. Omitted document regions are marked with `[[[ TRUNCATED: ... ]]]`. The selection is also supplied separately through `{{selection}}`, so custom profiles can place it wherever they want.

`max_chars` is a soft character budget, not a tokenizer count. Windows are aligned outward to line boundaries, so the final context can exceed the budget by up to one line. This keeps prompts readable and avoids splitting words or UTF-8 characters. Exact token accounting is deferred until a model-specific tokenizer is available.

When no document is supplied, `{{context}}` renders `(no surrounding document provided)`. A profile that declares `requires = ["context"]` fails instead.

## Prompt rendering

A profile's `user_prompt` is a strict template. The supported placeholders are `selection`, `context`, `question`, `file`, `response_language`, and `target_language`. There is no conditional logic and no loop syntax. Unknown placeholders are rejected when the profile loads.

Rendering scans the template only. Values inserted for one placeholder are not scanned again, so a document containing `{{question}}` cannot cause a second substitution. This is a small but deliberate defense against prompt injection through the text being read.

The built-in system prompt states that the document and selection are data, not instructions. A profile can replace the system prompt with `system_prompt`, but the default is designed to keep the model from following directives embedded in the reading material.

## Providers

Docent implements two wire formats directly:

- OpenAI Chat Completions, used by OpenAI, OpenCode Console, OpenCode Go, OpenRouter, local OpenAI-compatible servers, and similar gateways.
- Anthropic Messages, used by Anthropic and OpenCode's Anthropic-compatible endpoint.

The OpenAI-compatible endpoint at `https://opencode.ai/inference/go/openai/v1` and the Anthropic-compatible endpoint at `https://opencode.ai/inference/anthropic` are documented by OpenCode. See [OpenCode Go](https://opencode.ai/docs/console/go/) and [OpenCode Console models](https://opencode.ai/docs/zen/).

Requests include `User-Agent: docent/<version>`. For OpenCode endpoints, Docent also sends a stable `x-opencode-session` value from the platform state directory, which OpenCode recommends for routing and prompt caching.

API keys come from the environment variable named by `provider.api_key_env`, or from the optional `provider.api_key` field for users who accept a plaintext secret in `config.toml`. A set environment variable wins, `init` creates the file with mode `0600` on Unix, and keys are never included in error messages or `Debug` output. The CLI does not use a provider SDK because the two request shapes are small and a hand-written client keeps streaming, error mapping, and request inspection under direct control.

There are no retries in 0.1.0. A failed request returns a clear error instead of risking duplicate charges. Retry and backoff are deferred.

## NDJSON streaming contract

With `docent request --stream`, stdout contains one JSON object per line:

```json
{"type":"delta","text":"Hello"}
{"type":"delta","text":" world"}
{"type":"html","html":"<p>Hello world</p>\n"}
{"type":"done","model":"test-model","usage":{"input_tokens":3,"output_tokens":4}}
```

The `html` event is emitted only with `--html` (which requires `--stream`), after the last delta and before `done`. It is the CLI-rendered Markdown answer: raw HTML is escaped, images stay alt text, and only `http`, `https`, and `mailto` links are active. The qutebrowser, Firefox, and zathura adapters use it, so they no longer carry their own renderers.

Errors emitted after streaming starts use the same channel:

```json
{"type":"error","message":"provider request failed with HTTP 401: bad key"}
```

The CLI also writes the error to stderr and exits with the provider exit code. Neovim, qutebrowser, and Firefox parse complete lines only, so a chunk boundary can never split a JSON object or a UTF-8 character. The parser handles SSE `data:` frames that are split at arbitrary byte boundaries.

The stream ends with `done` or an error. A stream that ends without either is treated as a failure by the adapter if the child process exits non-zero, or as a completed answer if it exits zero.

## Errors and exit codes

| Code | Meaning |
|---|---|
| `0` | Success. |
| `2` | Configuration or usage error: bad TOML, missing API key variable, invalid settings, missing profile. |
| `3` | Provider or network error: HTTP error, timeout, malformed response, empty response. |
| `4` | Invalid input: malformed request JSON, missing required selection or question, unreadable document, empty document, phrase not found, line range out of bounds. |

Diagnostics and prompt labels go to stderr. The answer goes to stdout. A CLI invocation that is piped into another program can therefore keep the two separate.

## Security boundaries

- Profiles are loaded from `profiles/<name>.toml`. Names are restricted to letters, digits, underscores, and hyphens, so `--profile ../secret` cannot escape the directory.
- `vim.system` and `QProcess` receive argument arrays, not shell command lines, so buffer and page text cannot become shell syntax. Terminal commands likewise treat file paths and selection text as data, never as shell input. The Firefox extension uses `tabs.executeScript` with a fixed snippet and never builds a command line from page text.
- The Firefox host manifest lists the add-on ID that may connect, and the host refuses to start a program whose basename does not begin with `docent`. The extension requests `activeTab`, not host permissions, so it can read a page only after an explicit user action.
- API keys are referenced by environment variable name. The generated config contains no secrets, and the history database never stores a key.
- The history database stores full prompts and answers, including document context. It is local and unencrypted, and on Unix it is created with mode `0600` under a `0700` default data directory.
- Error messages include provider messages and truncated response bodies, never request headers.
- The document is treated as untrusted content. The default system prompt tells the model not to follow instructions inside it.
- The CLI browser viewer binds only to `127.0.0.1`, uses a random 128-bit token path, serves only the answer page and its event stream, and closes when the page closes.
- The shared Markdown renderer treats model output as untrusted: raw HTML is escaped, images are replaced by alt text, and links are limited to `http`, `https`, and `mailto`.

## Alternatives considered

- **Markdown profiles with a separate metadata block** would be pleasant for prose, but splits one profile across two files or requires a custom front-matter parser. Typed TOML keeps metadata and prompt together and reuses the same parser as `config.toml`.
- **`reqwest` with Tokio** would support advanced HTTP features, but the CLI is a one-shot process and the editor and browser adapters already provide asynchrony. Blocking `ureq` with rustls keeps the dependency graph small and avoids an OpenSSL system dependency.
- **Provider SDKs** would reduce wire-format code, but they add large dependency trees and their streaming types differ. Two small clients are easier to test and to keep compatible with OpenAI-style gateways.
- **Raw text streaming** would be simpler for the Lua side, but chunk boundaries can split UTF-8 sequences and multi-byte characters. NDJSON gives the plugin complete, decodable units.
- **Temporary files for the request** would avoid stdin handling, but they add cleanup, permissions, and race concerns. `vim.system` already supports stdin strings.
- **A generic command provider** would unlock subscription-only tools, but its output contracts, permissions, and process lifecycle vary too much for the first release.
- **A full-screen terminal UI or REPL** would duplicate the Neovim experience and add a terminal rendering stack. Plain commands with prompts cover the terminal use case with much less surface area.
- **A qutebrowser userscript** is the documented extension mechanism and needs no internal API, but it cannot stream into a live view or prompt cleanly for the ask profile. The adapter uses a Python module instead and accepts the compatibility risk.
- **A Firefox extension that calls providers directly** would avoid the native host, but it duplicates configuration, profiles, prompt assembly, history, and both provider clients in JavaScript and puts the API key in browser storage. The host keeps the CLI as the single implementation.
- **A local HTTP daemon for Firefox** avoids the native messaging manifest but adds a long-running server, port discovery, and localhost security concerns. Native messaging mirrors the qutebrowser process model and cancels with the tab.
- **An answer tab instead of a float overlay** avoids qutebrowser's application-wide key filter and keeps native tab navigation. The overlay is deferred until the tab workflow is stable.
