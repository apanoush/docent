# Rust crate modules

The crate has one binary target, `src/main.rs`, and a library target, `src/lib.rs`. The binary initializes logging, calls `cli::run`, prints errors to stderr, and maps errors to exit codes. Everything else lives in library modules so it can be tested without a terminal.

## Module map

| Module | Responsibility |
|---|---|
| `cli` | Clap definitions, subcommand dispatch, stdin request reading, output modes, NDJSON writing. |
| `config` | Config directory resolution, `config.toml` parsing and validation, default file creation. |
| `defaults` | `include_str!` copies of the shipped `config.toml` and profiles. |
| `profile` | Profile schema, validation, placeholder parsing, discovery, and metadata for `docent profiles`. |
| `request` | The JSON request type and selection range resolution. |
| `context` | Context modes, windowing, line alignment, selection and truncation markers. |
| `prompt` | Effective settings precedence and strict placeholder rendering. |
| `provider` | Provider dispatch, shared HTTP code, error extraction, streaming types. |
| `provider::openai` | OpenAI Chat Completions request and response handling. |
| `provider::anthropic` | Anthropic Messages request and response handling. |
| `provider::stream` | Incremental SSE decoding. |
| `error` | `DocentError` and process exit codes. |

## Data flow

A `run` invocation follows this path:

```text
cli::run
  config::ensure_initialized
  profile::ProfileStore::load
  cli::read_request -> request::Request
  prompt::EffectiveSettings::resolve
  context::build -> context::ContextResult
  prompt::render -> prompt::Prompt
  provider::complete or provider::complete_streaming -> provider::Completion
```

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
```

## Config and profile loading

`config::resolve_config_dir` is a pure function that takes the CLI value and the environment value. Tests cover the precedence without mutating process globals. `config::ensure_initialized` creates the default files only when `config.toml` is missing; it does not recreate deleted profiles.

`profile::ProfileStore::discover` reads every `.toml` file, validates it, sorts valid profiles by name, and collects warnings for files that were skipped. An empty directory returns `NoProfiles`; a directory with only invalid files returns `NoValidProfiles` with the validation messages. The Neovim picker only sees valid profiles.

Profile validation covers TOML fields, numeric ranges, placeholder syntax, requirement consistency, and the conflict between `requires = ["context"]` and `context_mode = "none"`.

## Context windowing

`context::build` uses byte offsets from the request only when they match the selected text. It converts byte budgets to character counts with `char_indices`, so slicing never lands inside a UTF-8 sequence. Windows are aligned to line starts and ends. `max_chars` is a soft budget; line alignment can add up to one line.

## Provider clients

Both clients share `provider::post_json`, which builds a `ureq` agent, sets the timeout and user agent, sends JSON, and maps non-2xx responses through `provider_error_message`. The clients differ only in request shape, endpoint path, authentication headers, and response parsing.

Streaming clients parse SSE through `provider::stream::stream_sse`. The decoder handles chunks split at arbitrary byte boundaries, ignores comments and event names, and dispatches complete `data:` payloads. OpenAI events read `choices[0].delta.content`; Anthropic events read `content_block_delta.delta.text`.

## Tests

Rust tests are split into three layers:

- Unit tests in each module for parsing, validation, windowing, rendering, request bodies, response parsing, SSE decoding, and settings precedence.
- Integration tests in `tests/cli.rs` for `init`, `config-path`, and `profiles`.
- Integration tests in `tests/provider.rs` for OpenAI and Anthropic requests, streaming, HTTP errors, malformed responses, and input errors. They use a local `TcpListener` mock server in `tests/common/mod.rs`, so no test reaches the network.

The coverage gate is `cargo llvm-cov --workspace --all-targets --fail-under-lines 80`. Run `scripts/check.sh` for the full gate, including formatting, clippy, tests, coverage, and the Neovim tests.
