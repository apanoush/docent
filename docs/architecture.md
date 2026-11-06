# Architecture

Docent is a one-shot CLI with a terminal front end, an editor adapter, a browser adapter, and a document-viewer adapter. Neovim owns the buffer and the editor UI; qutebrowser owns the page and the browser UI; zathura owns the PDF view; the CLI owns configuration, input resolution, prompting, context assembly, and provider communication.

```text
Terminal input              Neovim buffer              qutebrowser page
FILE, stdin, --lines,       selection, document,       selection, innerText,
--select, --text            byte offsets, file name    page URL and title
        |                         |                          |
        v                         v                          v
source adapter and prompts      JSON on stdin          JSON on stdin
        |                         |                          |
        +-------------------------+--------------------------+
                                  |
                                  v
                           docent executor
                           |-- load config.toml
                           |-- load profiles/<name>.toml
                           |-- resolve effective settings
                           |-- build context window
                           |-- render prompt template
                           |-- call provider
                                  |
                                  v
                    plain text, JSON, or NDJSON events
                                  |
                    +-------------+-------------+
                    |             |             |
                    v             v             v
             terminal output  Neovim float  qutebrowser answer tab
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

The terminal commands (`explain`, `ask`, `translate`, `do`) build the same `Request` as the Neovim plugin. The `source` module handles the difference in input shapes:

- A file is read as the document. `-` means stdin.
- With no file and piped stdin, the piped text becomes the document.
- With no file and a terminal stdin, Docent prompts for a file.
- Line ranges are 1-based and inclusive and are converted to byte offsets through a line index.
- `--select` uses the first exact phrase occurrence and fails when the phrase is absent.
- `--text` supplies a selection without line numbers. Offsets are resolved when the text occurs in the document.
- CRLF is normalized to LF before the line index is built, so offsets and selections stay consistent.

Prompts use a small `Prompter` trait. The real implementation reads from stdin and writes labels to stderr; tests substitute a scripted implementation. Prompts only run when stdin is a terminal, so a piped document is never consumed as prompt input. The profile is loaded before input resolution, which lets its `requires` list drive file, selection, and question prompts.

## qutebrowser adapter

The adapter is a Python module loaded from qutebrowser's `config.py`. It captures the page selection with `window.getSelection()` and the rendered page text with `document.body.innerText` through `tab.run_js_async`. The page URL has its query and fragment removed before it is used as the `file` label. A profile that declares `requires = ["question"]` opens a `QInputDialog` before the request starts.

The adapter starts `docent run --profile NAME --stream --source qutebrowser` with `QProcess`, writes the request JSON to stdin, and parses stdout with the same NDJSON line parser as the Neovim plugin. The answer streams into a new tab. Closing the tab calls `terminate()` and then `kill()` if needed. The question travels in the JSON request rather than in argv so it does not appear in the process list.

qutebrowser does not expose a stable third-party plugin API. The loader only walks built-in components, and `qutebrowser/api/__init__.py` says the extension API is not exposed to third-party extensions yet. The adapter therefore imports internal modules and must be retested on qutebrowser upgrades. It also requires JavaScript for selection capture and answer rendering. `window.getSelection()` sees the top frame only, so iframes, shadow DOM, and PDF viewers are not supported in the first version.

The first UI is a tab rather than a floating window. qutebrowser's application-wide key filter handles keys before widgets, and `q` is bound to macro recording in normal mode, so a float-like overlay would need insert/passthrough mode or a plugin key filter. A tab keeps normal qutebrowser bindings, including `d` to close and `j`/`k` to scroll.

## Zathura adapter

Zathura has no third-party UI plugin API. Its plugin system loads document backends by MIME type, and the only scripting hooks are `zathurarc` commands, `exec`, and D-Bus. The adapter therefore uses a `zathurarc` binding:

```zathurarc
map <C-e> exec /path/to/docent-zathura explain $FILE $PAGE
```

`exec` expands `$FILE` and `$PAGE` and starts the helper directly, without a shell. The helper cannot receive the selection directly because `exec` has no `$SELECTION` placeholder. Zathura copies a mouse selection to the clipboard, so the helper reads it with `wl-paste`, `xclip`, or `xsel`. The workflow is mouse-first because zathura has no keyboard text selection.

The helper extracts the current page plus one page on each side with `pdftotext`, builds the same JSON request as the other adapters, and writes it to a private temporary file. It then starts a terminal window running `docent-zathura render REQUEST_FILE`. The render process reads and deletes the request file, runs `docent run --profile NAME --stream --source zathura`, prints the `delta` events, and waits for Enter before closing. This two-process split keeps the selection out of argv and gives the answer a window that stays open after the stream ends.

The answer is a terminal window rather than a zathura overlay. An overlay would require patching zathura's GTK/C core. The terminal window streams naturally, supports the CLI's existing output, and can be floated with a compositor rule. The first version is PDF only; DjVu, PostScript, and comic-book files need different text extractors.

## Output modes

Terminal commands stream plain text to stdout by default and flush after each fragment. `--no-stream` waits for the complete answer and prints it once. The Neovim, qutebrowser, and zathura integrations use `docent run --stream`, which emits newline-delimited JSON instead. `docent run --json` wraps the final answer in one JSON object.

The shared executor builds the context and prompt once, then selects the output mode. This keeps buffered, streamed, JSON, and prompt-preview paths on the same request and prompt code.

## Run history

When history is enabled, `cli::execute` records one row after the provider call returns or fails. The row stores the command, profile, source, structured request inputs, effective settings, the rendered system and user messages, the answer, token usage, duration, status, and error. Streamed runs record the partial answer when the stream fails. `--print-prompt` and failures before the provider call are not recorded.

`history` opens the SQLite database once per run, enables WAL mode, applies a busy timeout, and creates the schema on first use. Schema changes are tracked with `PRAGMA user_version`. The database path comes from `history.database` or the platform data directory. A failed write is logged as a warning and never changes the command result.

The global `--source` flag, default `cli`, identifies the caller. The adapters pass `nvim`, `qutebrowser`, or `zathura`; custom plugins pass their own name. The source is only stored in history and does not affect request handling.

The database contains full prompts and answers, including document context, so it is local, unencrypted, and restricted to the owning user on Unix. API keys are not stored. See [run history](history.md) for the schema and queries.

## Settings and precedence

Settings resolve in this order, highest first:

1. CLI flags (`--context-mode`, `--target-language`).
2. Profile fields.
3. `config.toml`.
4. Built-in defaults.

Provider selection, base URL, API key environment variable, and timeout are global. Model, temperature, token limit, context mode, and languages can be overridden per profile. Keeping provider selection global avoids mixing credentials and wire formats in a single request.

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

API keys come from the environment variable named by `api.api_key_env`, or from the optional `api.api_key` field for users who accept a plaintext secret in `config.toml`. A set environment variable wins, `init` creates the file with mode `0600` on Unix, and keys are never included in error messages or `Debug` output. The CLI does not use a provider SDK because the two request shapes are small and a hand-written client keeps streaming, error mapping, and request inspection under direct control.

There are no retries in 0.1.0. A failed request returns a clear error instead of risking duplicate charges. Retry and backoff are deferred.

## NDJSON streaming contract

With `docent run --stream`, stdout contains one JSON object per line:

```json
{"type":"delta","text":"Hello"}
{"type":"delta","text":" world"}
{"type":"done","model":"test-model","usage":{"input_tokens":3,"output_tokens":4}}
```

Errors emitted after streaming starts use the same channel:

```json
{"type":"error","message":"provider request failed with HTTP 401: bad key"}
```

The CLI also writes the error to stderr and exits with the provider exit code. Neovim and qutebrowser parse complete lines only, so a chunk boundary can never split a JSON object or a UTF-8 character. The parser handles SSE `data:` frames that are split at arbitrary byte boundaries.

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
- `vim.system` and `QProcess` receive argument arrays, not shell command lines, so buffer and page text cannot become shell syntax. Terminal commands likewise treat file paths and selection text as data, never as shell input.
- API keys are referenced by environment variable name. The generated config contains no secrets, and the history database never stores a key.
- The history database stores full prompts and answers, including document context. It is local and unencrypted, and on Unix it is created with mode `0600` under a `0700` default data directory.
- Error messages include provider messages and truncated response bodies, never request headers.
- The document is treated as untrusted content. The default system prompt tells the model not to follow instructions inside it.

## Alternatives considered

- **Markdown profiles with a separate metadata block** would be pleasant for prose, but splits one profile across two files or requires a custom front-matter parser. Typed TOML keeps metadata and prompt together and reuses the same parser as `config.toml`.
- **`reqwest` with Tokio** would support advanced HTTP features, but the CLI is a one-shot process and the editor and browser adapters already provide asynchrony. Blocking `ureq` with rustls keeps the dependency graph small and avoids an OpenSSL system dependency.
- **Provider SDKs** would reduce wire-format code, but they add large dependency trees and their streaming types differ. Two small clients are easier to test and to keep compatible with OpenAI-style gateways.
- **Raw text streaming** would be simpler for the Lua side, but chunk boundaries can split UTF-8 sequences and multi-byte characters. NDJSON gives the plugin complete, decodable units.
- **Temporary files for the request** would avoid stdin handling, but they add cleanup, permissions, and race concerns. `vim.system` already supports stdin strings.
- **A generic command provider** would unlock subscription-only tools, but its output contracts, permissions, and process lifecycle vary too much for the first release.
- **A full-screen terminal UI or REPL** would duplicate the Neovim experience and add a terminal rendering stack. Plain commands with prompts cover the terminal use case with much less surface area.
- **A qutebrowser userscript** is the documented extension mechanism and needs no internal API, but it cannot stream into a live view or prompt cleanly for the ask profile. The adapter uses a Python module instead and accepts the compatibility risk.
- **An answer tab instead of a float overlay** avoids qutebrowser's application-wide key filter and keeps native tab navigation. The overlay is deferred until the tab workflow is stable.
