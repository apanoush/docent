# Rust crate modules

The crate has one binary target, `src/main.rs`, and a library target, `src/lib.rs`. The binary initializes logging, calls `cli::run`, prints errors to stderr, and maps errors to exit codes. Everything else lives in library modules so it can be tested without a terminal.

## Module map

| Module | Responsibility |
|---|---|
| `cli` | Clap definitions, subcommand dispatch, terminal prompts, stdin request reading, output modes, NDJSON writing. |
| `config` | Config directory resolution, `config.toml` parsing and validation, default file creation. |
| `defaults` | `include_str!` copies of the shipped `config.toml`, `answer.css`, and profiles. |
| `profile` | Profile schema, validation, placeholder parsing, discovery, and metadata for `docent profiles`. |
| `request` | The JSON request type and selection range resolution. |
| `source` | Terminal input adapters: file and stdin documents, line ranges, phrase and text selection, prompts, and request construction. |
| `context` | Context modes, windowing, line alignment, selection and truncation markers. |
| `prompt` | Effective settings precedence and strict placeholder rendering. |
| `provider` | Provider dispatch, shared HTTP code, error extraction, streaming types. |
| `provider::openai` | OpenAI Chat Completions request and response handling. |
| `provider::anthropic` | Anthropic Messages request and response handling. |
| `provider::stream` | Incremental SSE decoding. |
| `history` | SQLite run history, schema creation, path resolution, and inserts. |
| `error` | `DocentError` and process exit codes. |

## Data flow

A terminal command follows this path:

```text
cli::run
  config::ensure_initialized
  profile::ProfileStore::load
  prompt::EffectiveSettings::resolve
  source::resolve_request -> request::Request
  cli::execute
    context::build -> context::ContextResult
    prompt::render -> prompt::Prompt
    provider::complete or provider::complete_streaming
    history::write -> SQLite
```

`docent run` replaces `source::resolve_request` with `cli::read_request`, which parses the JSON document from stdin. Both paths converge on `cli::execute`.

`cli::execute` records one `history::NewRun` after the provider call returns or fails, unless `--no-history` or `history.enabled = false` skips it. Streamed output is accumulated so a failed stream still records the partial answer. A history write error is logged and ignored.

`--print-prompt` stops after `prompt::render` and serializes the system and user messages. Tests use this path to verify context and precedence without network access.

## Key interfaces

```rust
// config
pub fn resolve_config_dir(explicit: Option<PathBuf>, env_value: Option<OsString>) -> Result<PathBuf, DocentError>;
pub fn ensure_initialized(config_dir: &Path) -> Result<Config, DocentError>;
pub fn init(config_dir: &Path, force: bool) -> Result<InitReport, DocentError>;

// profile
pub fn template_placeholders(template: &str) -> Result<Vec<String>, String>;
pub fn is_valid_profile_name(name: &str) -> bool;
impl ProfileStore {
    pub fn load(&self, name: &str) -> Result<Profile, DocentError>;
    pub fn discover(&self) -> Result<ProfileCatalog, DocentError>;
}

// source
pub fn resolve_request(
    options: RequestOptions<'_>,
    prompter: &mut dyn Prompter,
    stdin_is_terminal: bool,
    stdin: &mut dyn Read,
) -> Result<Request, DocentError>;

// context
pub fn build(request: &Request, options: ContextOptions) -> ContextResult;

// prompt
impl EffectiveSettings {
    pub fn resolve(config: &Config, profile: &Profile, overrides: &Overrides) -> Result<Self, DocentError>;
}
pub fn render(profile: &Profile, request: &Request, context: &ContextResult, settings: &EffectiveSettings) -> Result<Prompt, DocentError>;

// provider
pub fn complete(config: &Config, settings: &EffectiveSettings, prompt: &Prompt) -> Result<Completion, DocentError>;
pub fn complete_streaming(config: &Config, settings: &EffectiveSettings, prompt: &Prompt, on_delta: &mut dyn FnMut(&str) -> Result<(), DocentError>) -> Result<Completion, DocentError>;

// history
pub fn database_path(config_dir: &Path, history: &HistoryConfig) -> Result<PathBuf, DocentError>;
pub fn write(config: &Config, config_dir: &Path, run: &NewRun) -> Result<(), DocentError>;
```

## Config and profile loading

`config::resolve_config_dir` is a pure function that takes the CLI value and the environment value. Tests cover the precedence without mutating process globals. `config::config_dir_from_env` then expands `~` and environment variables in the resolved path. `config::ensure_initialized` creates the default files only when `config.toml` is missing; it does not recreate deleted profiles.

`profile::ProfileStore::discover` reads every `.toml` file, validates it, sorts valid profiles by name, and collects warnings for files that were skipped. An empty directory returns `NoProfiles`; a directory with only invalid files returns `NoValidProfiles` with the validation messages. The Neovim and qutebrowser pickers only see valid profiles.

Profile validation covers TOML fields, numeric ranges, placeholder syntax, requirement consistency, and the conflict between `requires = ["context"]` and `context_mode = "none"`.

## Context windowing

`context::build` uses byte offsets from the request only when they match the selected text. It converts byte budgets to character counts with `char_indices`, so slicing never lands inside a UTF-8 sequence. Windows are aligned to line starts and ends. `max_chars` is a soft budget; line alignment can add up to one line.

## Terminal input and output

`source::Document` normalizes CRLF and stores a byte range for every line. Line selections are sliced from the first line start to the last line end, so they include newlines between lines but not the trailing newline after the final line. `--select` uses the first exact phrase occurrence and reports the count when it is ambiguous; `--text` resolves offsets only when the text occurs in the document.

`source::resolve_request` reads a file or stdin, applies the profile's `requires` list, and prompts through the `Prompter` trait when stdin is a terminal. `TerminalPrompter` writes labels to stderr and reads one line from stdin. `NoPrompter` is used for piped input, so a document on stdin is never consumed as prompt answers.

`cli::execute` selects one of five output modes: plain streaming for terminal commands, plain buffered for `--no-stream`, JSON for `docent run --json`, NDJSON for `docent run --stream`, and prompt JSON for `--print-prompt`. Plain streaming writes each provider fragment directly and flushes it, then adds a final newline if needed. When the context was truncated and the request has no resolvable selection, `cli::execute` logs a warning before rendering, so a whole-document run never silently covers only part of the document.

## Provider clients

Both clients share `provider::post_json`, which builds a `ureq` agent, sets the timeout and user agent, sends JSON, and maps non-2xx responses through `provider_error_message`. The clients differ only in request shape, endpoint path, authentication headers, and response parsing.

Streaming clients parse SSE through `provider::stream::stream_sse`. The decoder handles chunks split at arbitrary byte boundaries, ignores comments and event names, and dispatches complete `data:` payloads. OpenAI events read `choices[0].delta.content`; Anthropic events read `content_block_delta.delta.text`.

## Tests

Rust tests are split into three layers:

- Unit tests in each module for parsing, validation, windowing, rendering, request bodies, response parsing, SSE decoding, settings precedence, line ranges, prompt sequencing, and terminal request construction.
- Integration tests in `tests/cli.rs` for `init`, `config-path`, `profiles`, and the bare getting-started output.
- Integration tests in `tests/provider.rs` for OpenAI and Anthropic requests, streaming, HTTP errors, malformed responses, terminal commands, file and stdin input, selection errors, and input errors. They use a local `TcpListener` mock server in `tests/common/mod.rs`, so no test reaches the network.

The coverage gate is `cargo llvm-cov --workspace --all-targets --fail-under-lines 80`. Run `scripts/check.sh` for the full gate, including formatting, clippy, tests, coverage, the Neovim tests, and the qutebrowser adapter tests. The script requires `cargo-llvm-cov`, Neovim, pytest, ruff, and ty on `PATH`.
