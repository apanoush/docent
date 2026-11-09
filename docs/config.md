# Configuration and profiles

Docent keeps its configuration in a single directory. The directory is chosen in this order:

1. The global `--config-dir PATH` flag.
2. The `DOCENT_CONFIG_DIR` environment variable.
3. The platform config directory, for example `~/.config/docent` on Linux.

A path from the flag or the variable may contain `~` or environment variables such as `$HOME`; it is expanded shell-style. An undefined variable is an error.

The directory contains:

```text
config.toml
answer.css
profiles/
  explain.toml
  ask.toml
  translate.toml
  summarize.toml
```

`docent init` creates the directory and the default files. It skips files that already exist and only overwrites the shipped defaults with `--force`. Profile files you add yourself are never removed.

## `answer.css`

`docent init` writes a commented example `answer.css` for browser answer styling. The CLI browser viewer reads it automatically for `--viewer qutebrowser` and `--viewer firefox`; the terminal viewer ignores it. Point a browser adapter at it with its custom CSS option, for example the qutebrowser adapter:

```python
docent_qutebrowser.setup(config, custom_css="~/.config/docent/answer.css")
```

The Firefox adapter reads the same file automatically through its native messaging host and applies it after the bundled styles. The CLI applies the file after the built-in styles and before `--answer-css`.

The default answer styles ship with the adapter, so the file only needs the rules you want to change. `docent init --force` replaces it like the other shipped defaults. See [the qutebrowser adapter](qutebrowser.md#customizing-the-answer-style) for the available variables and examples.

## `config.toml`

The generated file points at OpenCode Go:

```toml
[provider]
wire_format = "openai"
base_url = "https://opencode.ai/inference/go/openai/v1"
model = "deepseek-v4.1-flash"
api_key_env = "OPENCODE_API_KEY"
# api_key = "sk-..."
timeout_secs = 120
max_tokens = 1200
temperature = 0.3

[context]
mode = "auto"
max_chars = 32000

[response]
language = "auto"
target_language = "English"

[viewer]
default = "terminal"
qutebrowser = "qutebrowser"
qutebrowser_target = "window"
# qutebrowser_args = ["--qt-arg", "name", "qute-docent"]
firefox = "firefox"
firefox_target = "window"

[history]
enabled = true
# database = "~/docent.db"

[cache]
enabled = true
max_entries = 1000
```

### `[provider]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `wire_format` | `"openai"` or `"anthropic"` | `"openai"` | Wire format used for the request. |
| `base_url` | string | `"https://opencode.ai/inference/go/openai/v1"` | Provider root URL, without the endpoint path. |
| `model` | string | `"deepseek-v4.1-flash"` | Model identifier sent to the provider. |
| `api_key` | string | unset | Literal API key stored in the file. Prefer `api_key_env` when the file is shared. |
| `api_key_env` | string | `"OPENCODE_API_KEY"` | Environment variable that holds the API key. A non-empty value wins over `api_key`. |
| `timeout_secs` | integer | `120` | Whole-request timeout in seconds. Must be greater than zero. |
| `max_tokens` | integer | `1200` | Maximum output tokens. Must be greater than zero. |
| `temperature` | float | `0.3` | Sampling temperature from 0.0 to 2.0. Anthropic requests must stay at or below 1.0. |

For `wire_format = "openai"`, Docent posts to `<base_url>/chat/completions`. For `wire_format = "anthropic"`, Docent posts to `<base_url>/v1/messages`, or to `<base_url>/messages` when the base URL already ends in `/v1`.

Requests are sent with `User-Agent: docent/<version>`. When the base URL contains `opencode.ai`, Docent also sends a stable `x-opencode-session` value from the platform state directory, which OpenCode uses for routing and prompt caching.

### API keys

Docent reads the key from the environment variable named by `api_key_env` when that variable exists and is not empty. Otherwise it uses `api_key`. If neither yields a value, the request fails with `missing API key`.

Storing the key in `api_key` keeps a secret in a plaintext file. `docent init` creates `config.toml` with mode `0600` on Unix, and Docent warns when a file containing `api_key` is readable by other users. Use `api_key_env` on shared machines and in dotfiles repositories, where the config file may be copied or committed.

Every provider example below can replace `api_key_env = "..."` with a literal key:

```toml
[provider]
wire_format = "openai"
base_url = "https://api.openai.com/v1"
model = "your-model-id"
api_key = "sk-..."
timeout_secs = 120
max_tokens = 1200
temperature = 0.3
```

### Provider examples

OpenAI direct:

```toml
[provider]
wire_format = "openai"
base_url = "https://api.openai.com/v1"
model = "your-model-id"
api_key_env = "OPENAI_API_KEY"
timeout_secs = 120
max_tokens = 1200
temperature = 0.3
```

Anthropic direct:

```toml
[provider]
wire_format = "anthropic"
base_url = "https://api.anthropic.com"
model = "your-model-id"
api_key_env = "ANTHROPIC_API_KEY"
timeout_secs = 120
max_tokens = 1200
temperature = 0.3
```

OpenCode Console, the pay-as-you-go endpoint:

```toml
[provider]
wire_format = "openai"
base_url = "https://opencode.ai/inference/openai/v1"
model = "deepseek-v4.1-flash"
api_key_env = "OPENCODE_API_KEY"
```

Local OpenAI-compatible server, for example Ollama or LM Studio:

```toml
[provider]
wire_format = "openai"
base_url = "http://localhost:11434/v1"
model = "your-model-id"
api_key_env = "LOCAL_API_KEY"
```

A local server that does not check keys still needs a key source. Set `api_key = "none"` or export the environment variable with any non-empty value.

### `[context]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `mode` | `"auto"`, `"full"`, or `"none"` | `"auto"` | How much of the document to send. |
| `max_chars` | integer | `32000` | Soft character budget for `auto` mode. Must be greater than zero. |

`auto` sends the whole document when it fits within `max_chars`. Otherwise it sends a window around the selection, cut on line boundaries. `full` always sends the whole document and ignores `max_chars`. `none` sends no surrounding document.

### `[response]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `language` | string | `"auto"` | Response language. `"auto"` tells the model to match the selected passage. Any other value is inserted into the prompt verbatim. |
| `target_language` | string | `"English"` | Default translation target used by the translate profile. |

### `[viewer]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `default` | `"terminal"`, `"qutebrowser"`, or `"firefox"` | `"terminal"` | Viewer used by `explain`, `ask`, `translate`, `summarize`, and `do` when `--viewer` is not passed. |
| `qutebrowser` | string | `"qutebrowser"` | Qutebrowser executable or command name. |
| `qutebrowser_target` | string | `"window"` | Qutebrowser `--target` value: `auto`, `tab`, `tab-bg`, `tab-silent`, `tab-bg-silent`, `window`, or `private-window`. |
| `qutebrowser_args` | list of strings | `[]` | Extra argv entries passed to qutebrowser after `--target` and before the answer URL. They are not shell-interpreted. |
| `firefox` | string | `"firefox"` | Firefox executable or command name. |
| `firefox_target` | string | `"window"` | Firefox URL target: `auto`, `tab`, `window`, or `private-window`. |

A partial table is valid, so `[viewer]\ndefault = "firefox"` is enough. The other keys keep their built-in defaults. Command-line flags still win, for example `--viewer terminal` overrides the configured default for one run. Repeated `--qutebrowser-arg` flags replace `qutebrowser_args` for that run. Each entry in `qutebrowser_args` becomes one argv entry, is inserted after `--target` and before the answer URL, and is never interpreted by a shell.

The viewer default applies to the terminal-facing commands. `docent request` keeps `terminal` as its default because it is the machine interface used by the Neovim, qutebrowser, Firefox, and zathura adapters. Pass `--viewer` to `docent request` to open a browser page explicitly; the configured executable and target still apply.

### `[history]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `enabled` | boolean | `true` | Record every provider run in a local SQLite database. |
| `database` | string | unset | Database path. The default is the platform data directory, for example `~/.local/share/docent/docent.db` on Linux. |

An explicit `database` path wins over the platform data directory. `~` and environment variables such as `$HOME` or `${XDG_DATA_HOME}` are expanded shell-style, and a relative path is resolved against the config directory. A path that references an undefined variable is reported as a warning and history is skipped for that run. The database is created lazily on the first model run; `docent init` does not create it.

The database contains the full rendered prompt, including document context, the question, and the answer. Treat it as sensitive. On Unix, Docent creates the file with mode `0600` and the default data directory with mode `0700`. It is not encrypted.

History is fail-open: when the database cannot be opened or written, Docent logs a warning and still returns the answer. Disable recording globally with `enabled = false`, or skip one run with the global `--no-history` flag. The global `--source NAME` flag records where a request came from; it defaults to `cli`, and the shipped adapters pass `nvim`, `qutebrowser`, `zathura`, or `firefox`.

See [run history](history.md) for the schema and example queries.

### `[cache]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `enabled` | boolean | `true` | Reuse stored answers for identical requests. |
| `max_entries` | integer | `1000` | Maximum number of cached answers. Least recently used entries are evicted beyond this. Must be greater than zero. |

The cache reuses an answer when the rendered system and user messages, the wire format, the base URL, the model, the temperature, and the maximum output token count all match. Because the rendered prompt contains the selection, the context window, the question, and the languages, changing any of those produces a miss. Byte offsets, the command name, the profile name, the source, and the output mode are not part of the key, so a stored answer can serve a different adapter or output mode.

Hits are fast and work without an API key, which makes previously seen content available offline. The first stored answer wins at a nonzero temperature; use `--refresh` to ask again and replace it, or `--no-cache` to skip both reading and writing. The two flags conflict with each other.

The cache lives in the same SQLite database as history and is disabled when history is disabled: with `history.enabled = false` or `--no-history`, Docent neither reads nor writes cached answers. A cache hit is not recorded as a history row, but it increments the entry's `hits` counter and updates `last_used_at`. See [run history](history.md) for the table and queries.


## Profiles

A profile is one TOML file in `profiles/`. The filename without `.toml` is the profile name, so `profiles/explain.toml` is invoked as `docent request --profile explain`. Profile names may contain letters, digits, underscores, and hyphens.

### Profile schema

| Key | Type | Required | Meaning |
|---|---|---|---|
| `user_prompt` | string | yes | User message template. Must not be empty. |
| `system_prompt` | string | no | Replaces the built-in system message for this profile. |
| `description` | string | no | Short description shown by the Neovim, qutebrowser, and Firefox profile pickers. |
| `requires` | array | no | Inputs required before the profile can run. Values are `selection`, `question`, and `context`. |
| `context_mode` | `"auto"`, `"full"`, or `"none"` | no | Per-profile context mode override. |
| `model` | string | no | Per-profile model override. |
| `temperature` | float | no | Per-profile temperature override. |
| `max_tokens` | integer | no | Per-profile output token limit override. |
| `response_language` | string | no | Per-profile response language override. |
| `target_language` | string | no | Per-profile translation target override. |

Unknown fields are rejected, so typos fail loudly instead of being ignored.

### Placeholders

| Placeholder | Value |
|---|---|
| `{{selection}}` | The selected passage. |
| `{{context}}` | Document context with selection markers, or a fallback note when no document was provided. |
| `{{question}}` | The follow-up question. |
| `{{file}}` | The file name, or `(unknown file)`. |
| `{{response_language}}` | The configured response language, or the phrase `the same language as the source text` when set to `auto`. |
| `{{target_language}}` | The configured translation target. |

Placeholders use `{{name}}`; whitespace inside the braces is allowed. Unknown placeholders are rejected at load time. Substituted values are not scanned again, so a document that contains `{{question}}` cannot inject another placeholder.

If a prompt uses `{{selection}}` or `{{question}}`, the profile must also declare the matching entry in `requires`. This lets the Neovim, qutebrowser, and Firefox adapters prompt for a question before starting the request. A profile may use `{{context}}` without declaring `requires = ["context"]`; in that case an empty document renders the fallback note instead of failing.

Each declared requirement must be used by `user_prompt` or `system_prompt`. Duplicate requirements are rejected. A profile that requires `context` cannot be combined with an effective `context_mode = "none"`.

### Precedence

Settings resolve in this order, highest first:

1. CLI flags (`--context-mode`, `--target-language`, and the viewer flags).
2. The profile file.
3. `config.toml`.
4. Built-in defaults.

`model`, `temperature`, and `max_tokens` have no CLI flags in 0.1.0. Set them in the profile or in `config.toml`. Viewer settings resolve separately: the CLI flags win over the `[viewer]` table, and profiles do not carry viewer settings.

### Example profile

```toml
description = "Explain the selected passage in context"
requires = ["selection"]
context_mode = "auto"

user_prompt = """
The document below is provided as context. The reader has selected a passage and wants to understand it.

<document>
{{context}}
</document>

<selection>
{{selection}}
</selection>

Explain the selected passage in context. Define unfamiliar terms, unpack the argument or imagery, and give the background a careful reader would need. Be concise: 2-6 sentences unless the passage requires more. Respond in {{response_language}}. Use Markdown.
"""
```

### TOML string escaping

TOML basic multiline strings use `"""` and treat backslashes as escapes. A prompt containing LaTeX, regular expressions, or code with backslashes should use a literal multiline string instead:

```toml
user_prompt = '''
Explain the formula $\sum_i x_i$ in {{selection}}.
'''
```

The generated profiles use `"""` because their text contains no backslashes.

### Default profiles

`docent init` writes four profiles:

| Profile | Requires | Command | Purpose |
|---|---|---|---|
| `explain` | `selection` | `docent explain` | Explain the selected passage in context. |
| `ask` | `selection`, `question` | `docent ask` | Answer a question about the selection. |
| `translate` | `selection` | `docent translate` | Translate the selection and note nuances. |
| `summarize` | `context` | `docent summarize` | Summarize the document, or the selection when one is marked. |

`summarize` is the only built-in profile that does not require a selection. Run it on a file to summarize the whole document, or pass `--lines` to summarize a section. When a selection is present, its markers appear inside the document context, and the prompt asks for a summary of that passage instead of the whole document.

## Terminal usage

```bash
docent explain notes.md --lines 12:20
docent explain notes.md --line 12
docent explain notes.md --phrase "transformer"
docent explain notes.md --text "a pasted passage"
docent ask notes.md --lines 12:20 --question "Why does this work?"
docent translate notes.md --lines 12:20 --to French
docent summarize paper.md
docent summarize paper.md --lines 12:40
docent summarize paper.pdf
docent summarize paper.pdf --pages 1:4
docent extract paper.pdf --pages 2:4
docent do my-profile notes.md --lines 12:20
```

### Input rules

- `FILE` is the document used for context and for line, phrase, or page selection. Use `-` for stdin.
- With no `FILE` and piped stdin, the piped text is the document.
- With no `FILE` and a terminal stdin, Docent prompts for a file. `--text` can supply a selection without a document when the profile does not require context.
- `--lines START:END` is 1-based and inclusive. `--line N` is a single line.
- `--page N` and `--pages START:END` are 1-based, inclusive, and PDF-only. They scope the document before `--lines`, `--phrase`, or `--text` apply, and may be combined with those selection options.
- Inputs whose bytes start with `%PDF-` are decoded with the built-in extractor; other inputs must be UTF-8 text. PDFs without a text layer are rejected.
- `--phrase PHRASE` uses the first matching occurrence, trying an exact match before a whitespace-tolerant one. A missing phrase is an input error.
- `--text TEXT` supplies a pasted selection. If a file or piped stdin is present, that is the document, and offsets are resolved with the same exact-then-tolerant search when the text occurs there.
- Only one of `--lines`, `--line`, `--phrase`, and `--text` may be used.
- `docent extract FILE [--page N | --pages START:END]` prints the extracted document text, which is useful for checking PDF input.
- `--context-mode` and `--target-language` (alias `--to`) work on every terminal command.
- `--viewer qutebrowser` shows the answer in a qutebrowser window or tab instead of stdout. `--qutebrowser`, `--qutebrowser-target`, `--qutebrowser-arg`, and `--answer-css` tune the viewer. See [the browser viewer](viewer.md).
- `--viewer firefox` shows the answer in a Firefox window or tab instead. `--firefox`, `--firefox-target`, and `--answer-css` tune it.
- Without `--viewer`, these commands use `viewer.default` from `config.toml`, which defaults to `terminal`. Pass `--viewer terminal` to override a configured browser default.

Piped stdin is always the document, never prompt input. Prompts only appear when stdin is a terminal and a required input is missing. A profile that requires a question prompts for one; a profile that requires a selection prompts for a line range or pasted text.

When a run has no resolvable selection and the document is larger than `context.max_chars`, Docent sends only part of it and warns on stderr. `--context-mode full` sends the whole document, raising `context.max_chars` widens the window, and `--lines` or `--pages` narrow the subject.

A bare `docent` prints a short getting-started message. `docent profiles` prints `name: description`; `docent profiles --json` remains the machine-readable form used by the Neovim, qutebrowser, and Firefox pickers.

Terminal commands stream plain text by default. `--no-stream` waits for the complete answer and prints it once. `--viewer qutebrowser` or `--viewer firefox` streams the answer into a browser instead and cannot be combined with `--no-stream`. `--json` and `--stream` are reserved for `docent request`, where `--stream` means newline-delimited JSON events.

## Machine interface

`docent request` reads a JSON request from stdin:

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

All fields are optional, but most profiles need a selection. Unknown fields are rejected. `byte_start` and `byte_end` are UTF-8 byte offsets into `document`; they are used only when they match `selection` exactly, otherwise Docent falls back to the first matching occurrence, trying an exact match before a whitespace-tolerant search.

Output modes for `docent request`:

- Default: the answer as plain text.
- `--json`: one JSON object with `text`, `model`, and `usage`.
- `--stream`: newline-delimited JSON events of type `delta`, `done`, and `error`.
- `--html`: with `--stream`, add one `html` event with the rendered answer before `done`.
- `--viewer qutebrowser`: stream the answer into a qutebrowser page instead of stdout. `--header TEXT` sets the page label.
- `--viewer firefox`: stream the answer into a Firefox page instead of stdout. `--header TEXT` sets the page label.
- `--print-prompt`: the assembled system and user messages as JSON, without calling the provider.
- `--stream`, `--json`, and the browser viewers cannot be combined.

## CLI reference

```text
docent explain [FILE] [--lines START:END | --line N | --phrase PHRASE | --text TEXT]
               [--page N | --pages START:END] [--question TEXT]
               [--context-mode auto|full|none]
               [--target-language LANG | --to LANG] [--no-stream]
               [--viewer terminal|qutebrowser|firefox] [--qutebrowser PATH]
               [--qutebrowser-target TARGET] [--qutebrowser-arg ARG]
               [--firefox PATH] [--firefox-target TARGET] [--answer-css PATH]
docent ask [FILE] ...
docent translate [FILE] ...
docent summarize [FILE] ...
docent do PROFILE [FILE] ...
docent extract FILE [--page N | --pages START:END]
docent request [--profile explain] [--question TEXT] [--target-language LANG]
               [--context-mode auto|full|none] [--stream] [--html] [--json]
               [--viewer terminal|qutebrowser|firefox] [--qutebrowser PATH]
               [--qutebrowser-target TARGET] [--qutebrowser-arg ARG]
               [--firefox PATH] [--firefox-target TARGET] [--answer-css PATH]
               [--header TEXT] [--print-prompt] [--config-dir PATH]
docent profiles [--json]
docent init [--force] [--config-dir PATH]
docent config-path [--config-dir PATH]
```

`--config-dir PATH`, `--source NAME`, `--no-history`, `--no-cache`, and `--refresh` are global flags and work with every command. `--config-dir` selects the config directory; `--source` is recorded in the history database; `--no-history` skips the database for one run; `--no-cache` and `--refresh` control the answer cache.

The `...` above stands for the same selection and execution options as `explain`. `docent do` takes a profile name before the optional file.

Exit codes: `0` success, `2` configuration or usage error, `3` provider or network error, `4` invalid input or a missing required value.

`docent profiles --json` returns metadata for the picker:

```json
{
  "profiles": [
    {
      "name": "ask",
      "description": "Ask a question about the selected passage",
      "requires": ["selection", "question"]
    }
  ]
}
```

Invalid profile files are reported on stderr and omitted from the catalog. Any command that loads a profile by name fails with the exact parse or validation error.
