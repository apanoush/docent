# Configuration and profiles

Docent keeps its configuration in a single directory. The directory is chosen in this order:

1. The global `--config-dir PATH` flag.
2. The `DOCENT_CONFIG_DIR` environment variable.
3. The platform config directory, for example `~/.config/docent` on Linux.

The directory contains:

```text
config.toml
profiles/
  explain.toml
  ask.toml
  translate.toml
```

`docent init` creates the directory and the default files. It skips files that already exist and only overwrites the shipped defaults with `--force`. Profile files you add yourself are never removed.

## `config.toml`

The generated file points at OpenCode Go:

```toml
[api]
kind = "openai"
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

[history]
enabled = true
# database = "~/docent.db"
```

### `[api]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `kind` | `"openai"` or `"anthropic"` | `"openai"` | Wire format used for the request. |
| `base_url` | string | `"https://opencode.ai/inference/go/openai/v1"` | Provider root URL, without the endpoint path. |
| `model` | string | `"deepseek-v4.1-flash"` | Model identifier sent to the provider. |
| `api_key` | string | unset | Literal API key stored in the file. Prefer `api_key_env` when the file is shared. |
| `api_key_env` | string | `"OPENCODE_API_KEY"` | Environment variable that holds the API key. A non-empty value wins over `api_key`. |
| `timeout_secs` | integer | `120` | Whole-request timeout in seconds. Must be greater than zero. |
| `max_tokens` | integer | `1200` | Maximum output tokens. Must be greater than zero. |
| `temperature` | float | `0.3` | Sampling temperature from 0.0 to 2.0. Anthropic requests must stay at or below 1.0. |

For `kind = "openai"`, Docent posts to `<base_url>/chat/completions`. For `kind = "anthropic"`, Docent posts to `<base_url>/v1/messages`, or to `<base_url>/messages` when the base URL already ends in `/v1`.

Requests are sent with `User-Agent: docent/<version>`. When the base URL contains `opencode.ai`, Docent also sends a stable `x-opencode-session` value from the platform state directory, which OpenCode uses for routing and prompt caching.

### API keys

Docent reads the key from the environment variable named by `api_key_env` when that variable exists and is not empty. Otherwise it uses `api_key`. If neither yields a value, the request fails with `missing API key`.

Storing the key in `api_key` keeps a secret in a plaintext file. `docent init` creates `config.toml` with mode `0600` on Unix, and Docent warns when a file containing `api_key` is readable by other users. Use `api_key_env` on shared machines and in dotfiles repositories, where the config file may be copied or committed.

Every provider example below can replace `api_key_env = "..."` with a literal key:

```toml
[api]
kind = "openai"
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
[api]
kind = "openai"
base_url = "https://api.openai.com/v1"
model = "your-model-id"
api_key_env = "OPENAI_API_KEY"
timeout_secs = 120
max_tokens = 1200
temperature = 0.3
```

Anthropic direct:

```toml
[api]
kind = "anthropic"
base_url = "https://api.anthropic.com"
model = "your-model-id"
api_key_env = "ANTHROPIC_API_KEY"
timeout_secs = 120
max_tokens = 1200
temperature = 0.3
```

OpenCode Console, the pay-as-you-go endpoint:

```toml
[api]
kind = "openai"
base_url = "https://opencode.ai/inference/openai/v1"
model = "deepseek-v4.1-flash"
api_key_env = "OPENCODE_API_KEY"
```

Local OpenAI-compatible server, for example Ollama or LM Studio:

```toml
[api]
kind = "openai"
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

### `[history]`

| Key | Type | Default | Meaning |
|---|---|---|---|
| `enabled` | boolean | `true` | Record every provider run in a local SQLite database. |
| `database` | string | unset | Database path. The default is the platform data directory, for example `~/.local/share/docent/docent.db` on Linux. |

An explicit `database` path wins over the platform data directory. `~` is expanded, and a relative path is resolved against the config directory. The database is created lazily on the first model run; `docent init` does not create it.

The database contains the full rendered prompt, including document context, the question, and the answer. Treat it as sensitive. On Unix, Docent creates the file with mode `0600` and the default data directory with mode `0700`. It is not encrypted.

History is fail-open: when the database cannot be opened or written, Docent logs a warning and still returns the answer. Disable recording globally with `enabled = false`, or skip one run with the global `--no-history` flag. The global `--source NAME` flag records where a request came from; it defaults to `cli`, and the shipped adapters pass `nvim`, `qutebrowser`, or `zathura`.

See [run history](history.md) for the schema and example queries.

## Profiles

A profile is one TOML file in `profiles/`. The filename without `.toml` is the profile name, so `profiles/explain.toml` is invoked as `docent run --profile explain`. Profile names may contain letters, digits, underscores, and hyphens.

### Profile schema

| Key | Type | Required | Meaning |
|---|---|---|---|
| `user_prompt` | string | yes | User message template. Must not be empty. |
| `system_prompt` | string | no | Replaces the built-in system message for this profile. |
| `description` | string | no | Short description shown by the Neovim and qutebrowser profile pickers. |
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
| `{{response_language}}` | The configured response language, or the phrase `the same language as the selected passage` when set to `auto`. |
| `{{target_language}}` | The configured translation target. |

Placeholders use `{{name}}`; whitespace inside the braces is allowed. Unknown placeholders are rejected at load time. Substituted values are not scanned again, so a document that contains `{{question}}` cannot inject another placeholder.

If a prompt uses `{{selection}}` or `{{question}}`, the profile must also declare the matching entry in `requires`. This lets the Neovim and qutebrowser adapters prompt for a question before starting the request. A profile may use `{{context}}` without declaring `requires = ["context"]`; in that case an empty document renders the fallback note instead of failing.

Each declared requirement must be used by `user_prompt` or `system_prompt`. Duplicate requirements are rejected. A profile that requires `context` cannot be combined with an effective `context_mode = "none"`.

### Precedence

Settings resolve in this order, highest first:

1. CLI flags (`--context-mode`, `--target-language`).
2. The profile file.
3. `config.toml`.
4. Built-in defaults.

`model`, `temperature`, and `max_tokens` have no CLI flags in 0.1.0. Set them in the profile or in `config.toml`.

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

## Terminal usage

```bash
docent explain notes.md --lines 12:20
docent explain notes.md --line 12
docent explain notes.md --select "transformer"
docent explain --text "a pasted passage" --file notes.md
docent ask notes.md --lines 12:20 --question "Why does this work?"
docent translate notes.md --lines 12:20 --to French
docent do my-profile notes.md --lines 12:20
```

### Input rules

- `FILE` is the document used for context and for line or phrase selection. Use `-` for stdin.
- With no `FILE` and piped stdin, the piped text is the document.
- With no `FILE` and a terminal stdin, Docent prompts for a file. The exception is `--text`, which can supply a selection without any document.
- `--lines START:END` is 1-based and inclusive. `--line N` is a single line.
- `--select PHRASE` uses the first exact occurrence. A missing phrase is an input error.
- `--text TEXT` supplies a pasted selection. If a file or piped stdin is present, that is the document, and offsets are resolved when the text occurs there.
- Only one of `--lines`, `--line`, `--select`, and `--text` may be used.
- `--context-mode` and `--target-language` (alias `--to`) work on every terminal command.

Piped stdin is always the document, never prompt input. Prompts only appear when stdin is a terminal and a required input is missing. A profile that requires a question prompts for one; a profile that requires a selection prompts for a line range or pasted text.

A bare `docent` prints a short getting-started message. `docent profiles` prints `name: description`; `docent profiles --json` remains the machine-readable form used by the Neovim and qutebrowser pickers.

Terminal commands stream plain text by default. `--no-stream` waits for the complete answer and prints it once. `--json` and `--stream` are reserved for `docent run`, where `--stream` means newline-delimited JSON events.

## Machine interface

`docent run` reads a JSON request from stdin:

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

All fields are optional, but most profiles need a selection. Unknown fields are rejected. `byte_start` and `byte_end` are UTF-8 byte offsets into `document`; they are used only when they match `selection` exactly, otherwise Docent falls back to the first exact occurrence of the selection.

Output modes for `docent run`:

- Default: the answer as plain text.
- `--json`: one JSON object with `text`, `model`, and `usage`.
- `--stream`: newline-delimited JSON events of type `delta`, `done`, and `error`.
- `--print-prompt`: the assembled system and user messages as JSON, without calling the provider.
- `--stream` and `--json` cannot be combined.

## CLI reference

```text
docent explain [FILE] [--lines START:END | --line N | --select PHRASE | --text TEXT]
               [--question TEXT] [--context-mode auto|full|none]
               [--target-language LANG | --to LANG] [--no-stream]
docent ask [FILE] ...
docent translate [FILE] ...
docent do PROFILE [FILE] ...
docent run [--profile explain] [--question TEXT] [--target-language LANG]
           [--context-mode auto|full|none] [--stream] [--json]
           [--print-prompt] [--config-dir PATH]
docent profiles [--json]
docent init [--force] [--config-dir PATH]
docent config-path [--config-dir PATH]
```

`--source NAME` and `--no-history` are global flags and work with every command. `--source` is recorded in the history database; `--no-history` skips the database for one run.

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
