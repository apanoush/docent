# Run history

Docent records every provider call in a local SQLite database. A row contains the command and profile, the source of the request, the structured inputs, the effective parameters, the exact rendered prompt, the answer, token usage, duration, and the error when the call failed.

History is enabled by default. Disable it globally in `config.toml`:

```toml
[history]
enabled = false
```

Or skip one run with the global flag:

```bash
docent explain notes.md --lines 12:20 --no-history
```

## Location

The default database is `docent.db` in the platform data directory:

| Platform | Default path |
|---|---|
| Linux | `$XDG_DATA_HOME/docent/docent.db`, usually `~/.local/share/docent/docent.db` |
| macOS | `~/Library/Application Support/docent/docent.db` |
| Windows | `%APPDATA%\docent\docent.db` |

Override it with `history.database`:

```toml
[history]
database = "~/sync/docent.db"
```

`~` is expanded, and a relative path is resolved against the config directory. The database is created on the first model run. `docent init` only writes configuration and profiles.

## Source

The global `--source NAME` flag records where a request came from. It defaults to `cli`. The shipped adapters pass their own names:

- Neovim: `nvim`
- qutebrowser: `qutebrowser`
- zathura: `zathura`

A custom plugin can pass any short name, for example `--source my-plugin`. Names may contain letters, digits, `-`, `_`, and `.`.

## Schema

Schema version 1 has one table:

| Column | Type | Meaning |
|---|---|---|
| `id` | integer | Row identifier. |
| `created_at` | integer | Unix epoch milliseconds when the provider call started. |
| `duration_ms` | integer | Provider call duration in milliseconds. |
| `command` | text | `explain`, `ask`, `translate`, `do`, or `run`. |
| `profile` | text | Profile name. |
| `source` | text | `cli`, `nvim`, `qutebrowser`, `zathura`, or a custom name. |
| `output_mode` | text | `plain_stream`, `plain`, `json`, or `ndjson`. |
| `status` | text | `success` or `error`. |
| `error` | text | Error message on failure. |
| `file` | text | File name used for context. |
| `selection` | text | Selected passage. |
| `question` | text | Follow-up question. |
| `document_bytes` | integer | Size of the original document in bytes. |
| `byte_start` | integer | Selection start offset. |
| `byte_end` | integer | Selection end offset. |
| `context_mode` | text | Effective context mode. |
| `context_max_chars` | integer | Configured character budget. |
| `context_chars` | integer | Character count of the assembled context. |
| `context_truncated` | integer | `1` when part of the document was omitted. |
| `provider` | text | `openai` or `anthropic`. |
| `base_url` | text | Provider base URL. |
| `model` | text | Model reported by the provider, or the configured model on failure. |
| `temperature` | real | Sampling temperature. |
| `max_tokens` | integer | Maximum output tokens. |
| `timeout_secs` | integer | Provider timeout. |
| `response_language` | text | Effective response language. |
| `target_language` | text | Effective translation target. |
| `system_prompt` | text | Rendered system message. |
| `user_prompt` | text | Rendered user message, including document context. |
| `output` | text | Full answer, or the partial answer from a failed stream. |
| `input_tokens` | integer | Input tokens reported by the provider. |
| `output_tokens` | integer | Output tokens reported by the provider. |
| `docent_version` | text | Docent version that produced the row. |

Indexes cover `created_at`, `profile`, and `source`.

The raw document is not stored separately. The rendered `user_prompt` already contains the context window that the model received. `document_bytes` records the original size.

`--print-prompt` runs never reach the provider and are not recorded. Input errors that stop before the provider call are not recorded either.

## Example queries

Recent runs:

```bash
sqlite3 ~/.local/share/docent/docent.db \
  "SELECT datetime(created_at / 1000, 'unixepoch', 'localtime'), source, profile, model, status FROM runs ORDER BY id DESC LIMIT 20;"
```

Token usage by model:

```bash
sqlite3 ~/.local/share/docent/docent.db \
  "SELECT model, COUNT(*) AS runs, SUM(input_tokens) AS input, SUM(output_tokens) AS output FROM runs GROUP BY model ORDER BY runs DESC;"
```

Slowest runs:

```bash
sqlite3 ~/.local/share/docent/docent.db \
  "SELECT datetime(created_at / 1000, 'unixepoch', 'localtime'), profile, duration_ms FROM runs ORDER BY duration_ms DESC LIMIT 10;"
```

Full text for one row:

```bash
sqlite3 ~/.local/share/docent/docent.db \
  "SELECT system_prompt, user_prompt, output FROM runs WHERE id = 42;"
```

## Failure and privacy

History is fail-open. If the database cannot be opened or written, Docent logs a warning to stderr and returns the answer with the normal exit code. A failed history write never fails a model request.

The database is local and unencrypted. It contains document excerpts, questions, answers, and provider metadata. API keys are never stored. On Unix, Docent creates the database with mode `0600` and the default data directory with mode `0700`. A custom parent directory is not changed, so choose a private location when you override `history.database`.

There is no automatic pruning in version 0.1.0. Rows accumulate until you delete the database or remove rows yourself.
