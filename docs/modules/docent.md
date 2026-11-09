# Rust crate modules

The crate has two binary targets and a library target. `src/main.rs` is the CLI: it initializes logging, calls `cli::run`, prints errors to stderr, and maps errors to exit codes. `src/bin/docent-native-host.rs` is the Firefox native messaging bridge, which delegates to `native_host::run`. Everything else lives in library modules so it can be tested without a terminal or a browser.

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
| `history` | SQLite run history, schema creation and migration, path resolution, and inserts. |
| `cache` | Exact-match answer cache keyed on the rendered prompt: key construction, lookup, store, hit counters, and eviction. |
| `markdown` | Safe Markdown-to-HTML rendering shared by the CLI viewer and the `html` stream event. |
| `viewer` | Qutebrowser and Firefox answer viewer: page shell, loopback SSE server, token, launch, and cancellation. |
| `native_host` | Firefox native messaging framing, request validation, process spawning, line forwarding, `exec`, and `css`. |
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
    cache::lookup or provider::complete/complete_streaming
    cache::store -> SQLite
    history::write -> SQLite
```

`docent run` replaces `source::resolve_request` with `cli::read_request`, which parses the JSON document from stdin. Both paths converge on `cli::execute`.

`cli::execute` resolves a cache key after rendering and, unless the run used `--no-cache` or the cache is disabled, reuses a stored answer or stores the fresh one. A cache hit is delivered through the same delta callback as a provider stream and is not written to `runs`. Every provider call records one `history::NewRun` after it returns or fails, unless `--no-history` or `history.enabled = false` skips it. Streamed output is accumulated so a failed stream still records the partial answer. History and cache errors are logged and ignored.

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

// cache
pub fn key(config: &Config, settings: &EffectiveSettings, prompt: &Prompt) -> [u8; 32];
pub fn lookup(config: &Config, config_dir: &Path, key: &[u8; 32]) -> Result<Option<CachedAnswer>, DocentError>;
pub fn store(config: &Config, config_dir: &Path, key: &[u8; 32], model: Option<&str>, output: &str) -> Result<(), DocentError>;
impl State {
    pub fn resolve(config: &Config, record_history: bool, mode: Mode, settings: &EffectiveSettings, prompt: &Prompt) -> Self;
}

// native_host
pub fn read_message(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>>;
pub fn write_message(writer: &mut impl Write, payload: &[u8]) -> io::Result<()>;
pub fn serve(request: HostRequest, writer: &mut impl Write) -> io::Result<()>;
```

## Config and profile loading

`config::resolve_config_dir` is a pure function that takes the CLI value and the environment value. Tests cover the precedence without mutating process globals. `config::config_dir_from_env` then expands `~` and environment variables in the resolved path. `config::ensure_initialized` creates the default files only when `config.toml` is missing; it does not recreate deleted profiles.

`profile::ProfileStore::discover` reads every `.toml` file, validates it, sorts valid profiles by name, and collects warnings for files that were skipped. An empty directory returns `NoProfiles`; a directory with only invalid files returns `NoValidProfiles` with the validation messages. The Neovim and qutebrowser pickers only see valid profiles.

Profile validation covers TOML fields, numeric ranges, placeholder syntax, requirement consistency, and the conflict between `requires = ["context"]` and `context_mode = "none"`.

`config::ViewerConfig` supplies the default viewer, browser executables, and targets for the terminal commands. `cli::ViewerArgs` resolves each value with the CLI flag first and the config second. `docent run` only honors an explicit `--viewer`, so the configured browser default never affects adapters that call it with `--stream`.

## Context windowing

`context::build` uses byte offsets from the request only when they match the selected text. It converts byte budgets to character counts with `char_indices`, so slicing never lands inside a UTF-8 sequence. Windows are aligned to line starts and ends. `max_chars` is a soft budget; line alignment can add up to one line.

## Terminal input and output

`source::Document` normalizes CRLF and stores a byte range for every line. Line selections are sliced from the first line start to the last line end, so they include newlines between lines but not the trailing newline after the final line. `--select` uses the first exact phrase occurrence and reports the count when it is ambiguous; `--text` resolves offsets only when the text occurs in the document.

`source::resolve_request` reads a file or stdin, applies the profile's `requires` list, and prompts through the `Prompter` trait when stdin is a terminal. `TerminalPrompter` writes labels to stderr and reads one line from stdin. `NoPrompter` is used for piped input, so a document on stdin is never consumed as prompt answers.

`cli::execute` selects one of seven output modes: plain streaming for terminal commands, plain buffered for `--no-stream`, JSON for `docent run --json`, NDJSON for `docent run --stream`, NDJSON with a rendered `html` event for `docent run --stream --html`, prompt JSON for `--print-prompt`, and the qutebrowser or Firefox viewer for `--viewer`. Before dispatch it checks the answer cache. A hit is replayed as one delta through the same callback, so all streaming modes and the browser viewer work unchanged, and buffered modes skip the provider call entirely. Plain streaming writes each fragment directly and flushes it, then adds a final newline if needed. The browser viewer runs the same provider stream, sends deltas over Server-Sent Events, and renders the finished answer with `markdown::render_answer`. When the context was truncated and the request has no resolvable selection, `cli::execute` logs a warning before rendering, so a whole-document run never silently covers only part of the document.

`markdown::render` enables tables and strikethrough, converts raw HTML to escaped text, drops images to alt text, restricts links to `http`, `https`, and `mailto`, wraps tables in `.table-wrap`, and adds `target="_blank"` to links. `viewer::Viewer` binds `127.0.0.1:0`, serves a tokenized page and event stream, waits 30 seconds for qutebrowser or Firefox to connect, and treats a closed page as cancellation. `DocentError::ViewerClosed` maps to a successful exit and a history status of `cancelled`.

## Provider clients

Both clients share `provider::post_json`, which builds a `ureq` agent, sets the timeout and user agent, sends JSON, and maps non-2xx responses through `provider_error_message`. The clients differ only in request shape, endpoint path, authentication headers, and response parsing.

Streaming clients parse SSE through `provider::stream::stream_sse`. The decoder handles chunks split at arbitrary byte boundaries, ignores comments and event names, and dispatches complete `data:` payloads. OpenAI events read `choices[0].delta.content`; Anthropic events read `content_block_delta.delta.text`.

## Tests

Rust tests are split into four layers:

- Unit tests in each module for parsing, validation, windowing, rendering, request bodies, response parsing, SSE decoding, settings precedence, line ranges, prompt sequencing, terminal request construction, Markdown sanitization, cache keys and eviction, and viewer page/server behavior.
- Integration tests in `tests/cli.rs` for `init`, `config-path`, `profiles`, the bare getting-started output, and viewer flag conflicts.
- Integration tests in `tests/provider.rs` for OpenAI and Anthropic requests, streaming, HTTP errors, malformed responses, terminal commands, file and stdin input, selection errors, input errors, the `html` event, the qutebrowser and Firefox viewers with a fake browser, and cache reuse, bypass, refresh, invalidation, and uncached failures. They use a local `TcpListener` mock server in `tests/common/mod.rs`, so no test reaches the network.
- Native host tests in `src/native_host.rs` with a fake `docent` shell script for framing, partial reads, oversized messages, invalid requests, spawn failures, line forwarding, exit codes, `exec`, and `css`.

The coverage gate is `cargo llvm-cov --workspace --all-targets --fail-under-lines 80`. Run `scripts/check.sh` for the full gate, including formatting, clippy, tests, coverage, the Neovim tests, the qutebrowser adapter tests, the zathura tests, and the Firefox tests. The script requires `cargo-llvm-cov`, Neovim, pytest, ruff, ty, and Node on `PATH`; `web-ext` is optional and only adds a manifest lint.
