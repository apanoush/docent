//! Configuration loading, validation, and default file creation.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::defaults;
use crate::error::DocentError;
use crate::viewer::{AnswerTarget, FirefoxTarget, QutebrowserTarget};

/// Environment variable that overrides the config directory.
pub const CONFIG_DIR_ENV: &str = "DOCENT_CONFIG_DIR";

const CONFIG_FILE: &str = "config.toml";
const ANSWER_CSS_FILE: &str = "answer.css";
const PROFILES_DIR: &str = "profiles";

/// Top level `config.toml` structure.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Provider settings.
    #[serde(default)]
    pub provider: ProviderConfig,
    /// Context window settings.
    #[serde(default)]
    pub context: ContextConfig,
    /// Response language settings.
    #[serde(default)]
    pub response: ResponseConfig,
    /// Answer viewer defaults.
    #[serde(default)]
    pub viewer: ViewerConfig,
    /// Run history settings.
    #[serde(default)]
    pub history: HistoryConfig,
    /// Answer cache settings.
    #[serde(default)]
    pub cache: CacheConfig,
}

/// Wire format used to talk to the provider.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WireFormat {
    /// OpenAI Chat Completions, also used by OpenAI-compatible gateways.
    #[default]
    OpenAi,
    /// Anthropic Messages API.
    Anthropic,
}

impl WireFormat {
    /// Stable identifier used in the history database.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
        }
    }
}

/// Provider settings.
#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    /// Wire format.
    pub wire_format: WireFormat,
    /// Base URL, without the endpoint path.
    pub base_url: String,
    /// Model identifier.
    pub model: String,
    /// Literal API key. Prefer `api_key_env` when the config file is shared.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Environment variable holding the API key. Wins over `api_key` when set.
    #[serde(default)]
    pub api_key_env: String,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Maximum number of output tokens.
    pub max_tokens: u32,
    /// Sampling temperature.
    pub temperature: f32,
}

impl std::fmt::Debug for ProviderConfig {
    /// Prints every field except the key itself.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("wire_format", &self.wire_format)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("api_key_env", &self.api_key_env)
            .field("timeout_secs", &self.timeout_secs)
            .field("max_tokens", &self.max_tokens)
            .field("temperature", &self.temperature)
            .finish()
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            wire_format: WireFormat::OpenAi,
            base_url: "https://opencode.ai/inference/go/openai/v1".to_string(),
            model: "deepseek-v4.1-flash".to_string(),
            api_key: None,
            api_key_env: "OPENCODE_API_KEY".to_string(),
            timeout_secs: 120,
            max_tokens: 1200,
            temperature: 0.3,
        }
    }
}

/// How much of the surrounding document to send.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ContextMode {
    /// Use the whole document when it fits, otherwise a window around the selection.
    #[default]
    Auto,
    /// Always send the whole document.
    Full,
    /// Send no surrounding document.
    None,
}

impl ContextMode {
    /// Stable identifier used in the history database.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Full => "full",
            Self::None => "none",
        }
    }
}

/// Context window settings.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContextConfig {
    /// Context mode.
    pub mode: ContextMode,
    /// Maximum number of document characters sent to the model.
    pub max_chars: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            mode: ContextMode::Auto,
            max_chars: 32_000,
        }
    }
}

/// Response language settings.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResponseConfig {
    /// Response language, or `auto` to match the selected passage.
    pub language: String,
    /// Translation target language used by the translate profile.
    pub target_language: String,
}

impl Default for ResponseConfig {
    fn default() -> Self {
        Self {
            language: "auto".to_string(),
            target_language: "English".to_string(),
        }
    }
}

/// Answer viewer defaults used by the terminal commands.
///
/// Missing keys fall back to the built-in defaults, so a partial `[viewer]`
/// table such as `default = "firefox"` is valid.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ViewerConfig {
    /// Default viewer for the terminal commands.
    pub default: AnswerTarget,
    /// Qutebrowser executable or command name.
    pub qutebrowser: String,
    /// Qutebrowser `--target` value.
    pub qutebrowser_target: QutebrowserTarget,
    /// Extra arguments passed to qutebrowser before the answer URL.
    pub qutebrowser_args: Vec<String>,
    /// Firefox executable or command name.
    pub firefox: String,
    /// Firefox URL target.
    pub firefox_target: FirefoxTarget,
}

impl Default for ViewerConfig {
    fn default() -> Self {
        Self {
            default: AnswerTarget::Terminal,
            qutebrowser: "qutebrowser".to_string(),
            qutebrowser_target: QutebrowserTarget::Window,
            qutebrowser_args: Vec::new(),
            firefox: "firefox".to_string(),
            firefox_target: FirefoxTarget::Window,
        }
    }
}

/// Run history settings.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HistoryConfig {
    /// Whether provider runs are recorded.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Explicit database path. Relative paths resolve against the config directory.
    #[serde(default)]
    pub database: Option<PathBuf>,
}

fn default_true() -> bool {
    true
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            database: None,
        }
    }
}

/// Answer cache settings.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    /// Whether stored answers are reused for identical requests.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Maximum number of cached answers before least recently used entries are evicted.
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
}

fn default_max_entries() -> usize {
    1000
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: default_max_entries(),
        }
    }
}

impl Config {
    /// Checks values that TOML parsing cannot enforce.
    pub fn validate(&self, path: &Path) -> Result<(), DocentError> {
        let mut errors = Vec::new();

        if self.provider.base_url.trim().is_empty() {
            errors.push("provider.base_url must not be empty".to_string());
        } else if !self.provider.base_url.starts_with("http://")
            && !self.provider.base_url.starts_with("https://")
        {
            errors.push("provider.base_url must start with http:// or https://".to_string());
        }
        if self.provider.model.trim().is_empty() {
            errors.push("provider.model must not be empty".to_string());
        }
        if api_key_is_empty(self.provider.api_key.as_deref())
            && self.provider.api_key_env.trim().is_empty()
        {
            errors.push(
                "provider.api_key and provider.api_key_env must not both be empty".to_string(),
            );
        }
        if self.provider.timeout_secs == 0 {
            errors.push("provider.timeout_secs must be greater than zero".to_string());
        }
        if self.provider.max_tokens == 0 {
            errors.push("provider.max_tokens must be greater than zero".to_string());
        }
        if !(0.0..=2.0).contains(&self.provider.temperature) {
            errors.push(format!(
                "provider.temperature must be between 0.0 and 2.0, got {}",
                self.provider.temperature
            ));
        }
        if self.context.max_chars == 0 {
            errors.push("context.max_chars must be greater than zero".to_string());
        }
        if self.response.language.trim().is_empty() {
            errors.push("response.language must not be empty".to_string());
        }
        if self.response.target_language.trim().is_empty() {
            errors.push("response.target_language must not be empty".to_string());
        }
        if self.viewer.qutebrowser.trim().is_empty() {
            errors.push("viewer.qutebrowser must not be empty".to_string());
        }
        if self
            .viewer
            .qutebrowser_args
            .iter()
            .any(|arg| arg.is_empty())
        {
            errors.push("viewer.qutebrowser_args must not contain empty arguments".to_string());
        }
        if self
            .viewer
            .qutebrowser_args
            .iter()
            .any(|arg| arg.contains('\0'))
        {
            errors.push("viewer.qutebrowser_args must not contain NUL bytes".to_string());
        }
        if self.viewer.firefox.trim().is_empty() {
            errors.push("viewer.firefox must not be empty".to_string());
        }
        if self
            .history
            .database
            .as_ref()
            .is_some_and(|path| path.as_os_str().to_string_lossy().trim().is_empty())
        {
            errors.push("history.database must not be empty".to_string());
        }
        if self.cache.max_entries == 0 {
            errors.push("cache.max_entries must be greater than zero".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(DocentError::InvalidConfig {
                path: path.to_path_buf(),
                message: errors.join("; "),
            })
        }
    }
}

/// True when an optional API key is absent or blank.
fn api_key_is_empty(key: Option<&str>) -> bool {
    key.map(str::trim).unwrap_or("").is_empty()
}

/// Resolves the config directory from the CLI flag, environment, or platform default.
pub fn resolve_config_dir(
    explicit: Option<PathBuf>,
    env_value: Option<OsString>,
) -> Result<PathBuf, DocentError> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if let Some(value) = env_value.filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(value));
    }
    ProjectDirs::from("", "", "docent")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .ok_or(DocentError::NoConfigDir)
}

/// Resolves the config directory from the current process environment.
///
/// The resolved path is shell-expanded, so `DOCENT_CONFIG_DIR` and `--config-dir`
/// may contain `~` or environment variables.
pub fn config_dir_from_env(explicit: Option<PathBuf>) -> Result<PathBuf, DocentError> {
    let dir = resolve_config_dir(explicit, std::env::var_os(CONFIG_DIR_ENV))?;
    expand_path(&dir)
}

/// Expands `~` and environment variables in a configured path.
///
/// Paths that are not valid UTF-8 are returned unchanged, since shell
/// expansion only applies to text.
pub fn expand_path(path: &Path) -> Result<PathBuf, DocentError> {
    let Some(text) = path.to_str() else {
        return Ok(path.to_path_buf());
    };

    shellexpand::full(text)
        .map(|expanded| PathBuf::from(expanded.as_ref()))
        .map_err(|source| DocentError::ExpandPath {
            path: path.to_path_buf(),
            source,
        })
}

/// Path to `config.toml`.
pub fn config_file(config_dir: &Path) -> PathBuf {
    config_dir.join(CONFIG_FILE)
}

/// Path to the profiles directory.
pub fn profiles_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(PROFILES_DIR)
}

/// Path to the example answer stylesheet created by `docent init`.
pub fn answer_css_file(config_dir: &Path) -> PathBuf {
    config_dir.join(ANSWER_CSS_FILE)
}

/// Loads and validates `config.toml`.
pub fn load_config(config_dir: &Path) -> Result<Config, DocentError> {
    let path = config_file(config_dir);
    let contents = fs::read_to_string(&path).map_err(|source| DocentError::Read {
        path: path.clone(),
        source,
    })?;
    let config: Config = toml::from_str(&contents).map_err(|source| DocentError::ParseConfig {
        path: path.clone(),
        source,
    })?;
    config.validate(&path)?;
    warn_if_key_is_exposed(&path, &config);
    Ok(config)
}

/// Warns when a literal API key sits in a file other users can read.
#[cfg(unix)]
fn warn_if_key_is_exposed(path: &Path, config: &Config) {
    use std::os::unix::fs::PermissionsExt;

    if api_key_is_empty(config.provider.api_key.as_deref()) {
        return;
    }
    if let Ok(metadata) = fs::metadata(path) {
        if metadata.permissions().mode() & 0o077 != 0 {
            log::warn!(
                "{} contains an API key and is readable by other users; run 'chmod 600 {}'",
                path.display(),
                path.display()
            );
        }
    }
}

/// No-op on platforms without Unix permission bits.
#[cfg(not(unix))]
fn warn_if_key_is_exposed(_path: &Path, _config: &Config) {}

/// Creates default files if `config.toml` is missing, then loads the config.
pub fn ensure_initialized(config_dir: &Path) -> Result<Config, DocentError> {
    if !config_file(config_dir).exists() {
        log::info!(
            "no config at {}; creating defaults",
            config_file(config_dir).display()
        );
        init(config_dir, false)?;
    }
    load_config(config_dir)
}

/// Result of creating default configuration files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    /// Config directory that was initialized.
    pub config_dir: PathBuf,
    /// Files that were written.
    pub created: Vec<PathBuf>,
    /// Files that already existed and were left untouched.
    pub skipped: Vec<PathBuf>,
}

/// Creates the config directory, `config.toml`, the example `answer.css`, and
/// the default profiles.
///
/// Existing files are skipped unless `force` is set. Profiles not shipped as
/// defaults are never removed.
pub fn init(config_dir: &Path, force: bool) -> Result<InitReport, DocentError> {
    if config_dir.exists() && !config_dir.is_dir() {
        return Err(DocentError::ConfigDirNotDirectory(config_dir.to_path_buf()));
    }

    create_dir_all(config_dir)?;
    let profile_dir = profiles_dir(config_dir);
    create_dir_all(&profile_dir)?;

    let mut report = InitReport {
        config_dir: config_dir.to_path_buf(),
        created: Vec::new(),
        skipped: Vec::new(),
    };

    let config_path = config_file(config_dir);
    write_default(&config_path, defaults::CONFIG, force, &mut report)?;
    if report.created.contains(&config_path) {
        restrict_permissions(&config_path)?;
    }
    write_default(
        &answer_css_file(config_dir),
        defaults::ANSWER_CSS,
        force,
        &mut report,
    )?;
    for (name, contents) in defaults::PROFILES {
        write_default(
            &profile_dir.join(format!("{name}.toml")),
            contents,
            force,
            &mut report,
        )?;
    }

    Ok(report)
}

/// Limits a freshly created config file to its owner, since it may hold a key.
#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<(), DocentError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
        DocentError::Write {
            path: path.to_path_buf(),
            source,
        }
    })
}

/// No-op on platforms without Unix permission bits.
#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<(), DocentError> {
    Ok(())
}

fn create_dir_all(path: &Path) -> Result<(), DocentError> {
    fs::create_dir_all(path).map_err(|source| DocentError::CreateDir {
        path: path.to_path_buf(),
        source,
    })
}

fn write_default(
    path: &Path,
    contents: &str,
    force: bool,
    report: &mut InitReport,
) -> Result<(), DocentError> {
    if path.exists() && !force {
        report.skipped.push(path.to_path_buf());
        return Ok(());
    }
    fs::write(path, contents).map_err(|source| DocentError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    report.created.push(path.to_path_buf());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn default_config_is_valid() {
        let config: Config = toml::from_str(defaults::CONFIG).unwrap();
        config.validate(Path::new("config.toml")).unwrap();

        assert_eq!(config.provider.wire_format, WireFormat::OpenAi);
        assert_eq!(config.context.mode, ContextMode::Auto);
        assert_eq!(config.provider.model, "deepseek-v4.1-flash");
        assert!(config.history.enabled);
        assert!(config.history.database.is_none());
        assert_eq!(config.viewer, ViewerConfig::default());
    }

    #[test]
    fn viewer_config_defaults_to_the_terminal() {
        let config = Config::default();
        config.validate(Path::new("config.toml")).unwrap();

        assert_eq!(config.viewer.default, AnswerTarget::Terminal);
        assert_eq!(config.viewer.qutebrowser, "qutebrowser");
        assert_eq!(config.viewer.qutebrowser_target, QutebrowserTarget::Window);
        assert!(config.viewer.qutebrowser_args.is_empty());
        assert_eq!(config.viewer.firefox, "firefox");
        assert_eq!(config.viewer.firefox_target, FirefoxTarget::Window);
    }

    #[test]
    fn viewer_config_accepts_a_partial_table() {
        let config: Config = toml::from_str(
            r#"
[viewer]
default = "firefox"
"#,
        )
        .unwrap();
        config.validate(Path::new("config.toml")).unwrap();

        assert_eq!(config.viewer.default, AnswerTarget::Firefox);
        assert_eq!(config.viewer.firefox, "firefox");
        assert_eq!(config.viewer.qutebrowser_target, QutebrowserTarget::Window);
        assert!(config.viewer.qutebrowser_args.is_empty());
    }

    #[test]
    fn viewer_config_accepts_executables_and_targets() {
        let config: Config = toml::from_str(
            r#"
[viewer]
default = "qutebrowser"
qutebrowser = "/usr/bin/qutebrowser"
qutebrowser_target = "tab-bg"
qutebrowser_args = ["--qt-arg", "name", "qute-docent"]
firefox = "firefox-esr"
firefox_target = "private-window"
"#,
        )
        .unwrap();
        config.validate(Path::new("config.toml")).unwrap();

        assert_eq!(config.viewer.default, AnswerTarget::Qutebrowser);
        assert_eq!(config.viewer.qutebrowser, "/usr/bin/qutebrowser");
        assert_eq!(config.viewer.qutebrowser_target, QutebrowserTarget::TabBg);
        assert_eq!(
            config.viewer.qutebrowser_args,
            vec!["--qt-arg", "name", "qute-docent"]
        );
        assert_eq!(config.viewer.firefox, "firefox-esr");
        assert_eq!(config.viewer.firefox_target, FirefoxTarget::PrivateWindow);
    }

    #[test]
    fn viewer_config_rejects_unknown_fields() {
        let error = toml::from_str::<Config>("[viewer]\npath = \"firefox\"\n").unwrap_err();
        assert!(error.to_string().contains("path"));
    }

    #[test]
    fn viewer_config_rejects_an_unknown_viewer() {
        let error = toml::from_str::<Config>("[viewer]\ndefault = \"chromium\"\n").unwrap_err();
        assert!(error.to_string().contains("unknown variant"));
    }

    #[test]
    fn viewer_config_rejects_an_unknown_target() {
        let error =
            toml::from_str::<Config>("[viewer]\nqutebrowser_target = \"overlay\"\n").unwrap_err();
        assert!(error.to_string().contains("unknown variant"));
    }

    #[test]
    fn viewer_config_rejects_empty_executables() {
        let mut config = Config::default();
        config.viewer.qutebrowser.clear();
        config.viewer.firefox.clear();

        let error = config.validate(Path::new("config.toml")).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("viewer.qutebrowser"));
        assert!(message.contains("viewer.firefox"));
    }

    #[test]
    fn viewer_config_rejects_empty_qutebrowser_args() {
        let mut config = Config::default();
        config.viewer.qutebrowser_args.push(String::new());

        let error = config.validate(Path::new("config.toml")).unwrap_err();
        assert!(error.to_string().contains("viewer.qutebrowser_args"));
    }

    #[test]
    fn viewer_config_rejects_nul_qutebrowser_args() {
        let mut config = Config::default();
        config.viewer.qutebrowser_args.push("bad\0arg".to_string());

        let error = config.validate(Path::new("config.toml")).unwrap_err();
        assert!(error.to_string().contains("viewer.qutebrowser_args"));
    }

    #[test]
    fn history_config_accepts_a_database_path() {
        let config: Config = toml::from_str(
            r#"
[history]
enabled = true
database = "~/docent.db"
"#,
        )
        .unwrap();
        config.validate(Path::new("config.toml")).unwrap();

        assert!(config.history.enabled);
        assert_eq!(config.history.database, Some(PathBuf::from("~/docent.db")));
    }

    #[test]
    fn history_config_can_be_disabled() {
        let config: Config = toml::from_str("[history]\nenabled = false\n").unwrap();
        assert!(!config.history.enabled);
        assert!(config.history.database.is_none());
    }

    #[test]
    fn history_config_rejects_an_empty_database_path() {
        let config: Config = toml::from_str("[history]\ndatabase = \"\"\n").unwrap();
        let error = config.validate(Path::new("config.toml")).unwrap_err();
        assert!(error.to_string().contains("history.database"));
    }

    #[test]
    fn history_config_rejects_unknown_fields() {
        let error = toml::from_str::<Config>("[history]\npath = \"docent.db\"\n").unwrap_err();
        assert!(error.to_string().contains("path"));
    }

    #[test]
    fn cache_config_defaults_to_enabled() {
        let config = Config::default();
        config.validate(Path::new("config.toml")).unwrap();
        assert!(config.cache.enabled);
        assert_eq!(config.cache.max_entries, 1000);
    }

    #[test]
    fn cache_config_can_be_disabled() {
        let config: Config = toml::from_str("[cache]\nenabled = false\n").unwrap();
        config.validate(Path::new("config.toml")).unwrap();
        assert!(!config.cache.enabled);
        assert_eq!(config.cache.max_entries, 1000);
    }

    #[test]
    fn cache_config_rejects_zero_entries() {
        let config: Config = toml::from_str("[cache]\nmax_entries = 0\n").unwrap();
        let error = config.validate(Path::new("config.toml")).unwrap_err();
        assert!(error.to_string().contains("cache.max_entries"));
    }

    #[test]
    fn cache_config_rejects_unknown_fields() {
        let error = toml::from_str::<Config>("[cache]\nsize = 10\n").unwrap_err();
        assert!(error.to_string().contains("size"));
    }

    #[test]
    fn config_accepts_a_literal_api_key_without_an_env_name() {
        let config: Config = toml::from_str(
            r#"
[provider]
wire_format = "openai"
base_url = "https://example.com"
model = "test-model"
api_key = "secret"
timeout_secs = 5
max_tokens = 16
temperature = 0.1
"#,
        )
        .unwrap();
        config.validate(Path::new("config.toml")).unwrap();

        assert_eq!(config.provider.api_key.as_deref(), Some("secret"));
        assert!(config.provider.api_key_env.is_empty());
    }

    #[test]
    fn config_accepts_an_env_name_without_a_literal_key() {
        let config = Config::default();
        config.validate(Path::new("config.toml")).unwrap();

        assert!(config.provider.api_key.is_none());
        assert_eq!(config.provider.api_key_env, "OPENCODE_API_KEY");
    }

    #[test]
    fn config_rejects_both_missing_key_sources() {
        let mut config = Config::default();
        config.provider.api_key = None;
        config.provider.api_key_env.clear();

        let error = config.validate(Path::new("config.toml")).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("provider.api_key"));
        assert!(message.contains("provider.api_key_env"));
    }

    #[test]
    fn debug_output_redacts_the_api_key() {
        let mut config = Config::default();
        config.provider.api_key = Some("super-secret-value".to_string());

        let debug = format!("{:?}", config);
        assert!(!debug.contains("super-secret-value"));
        assert!(debug.contains("<redacted>"));
    }

    #[cfg(unix)]
    #[test]
    fn init_restricts_config_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("docent");
        init(&dir, false).unwrap();

        let mode = fs::metadata(config_file(&dir))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn unknown_config_fields_are_rejected() {
        let error = toml::from_str::<Config>("[provider]\nunknown = 1\n").unwrap_err();
        assert!(error.to_string().contains("unknown"));
    }

    #[test]
    fn invalid_context_mode_is_rejected() {
        let error = toml::from_str::<Config>("[context]\nmode = \"sideways\"\n").unwrap_err();
        assert!(error.to_string().contains("unknown variant"));
    }

    #[test]
    fn resolve_config_dir_prefers_explicit_value() {
        let path = resolve_config_dir(
            Some(PathBuf::from("/tmp/explicit")),
            Some(OsString::from("/tmp/from-env")),
        )
        .unwrap();
        assert_eq!(path, PathBuf::from("/tmp/explicit"));
    }

    #[test]
    fn resolve_config_dir_uses_environment() {
        let path = resolve_config_dir(None, Some(OsString::from("/tmp/from-env"))).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/from-env"));
    }

    #[test]
    fn config_dir_expands_a_leading_tilde() {
        let Some(dirs) = directories::BaseDirs::new() else {
            return;
        };
        let path = config_dir_from_env(Some(PathBuf::from("~/docent"))).unwrap();
        assert_eq!(path, dirs.home_dir().join("docent"));
    }

    #[test]
    fn config_dir_expands_environment_variables() {
        // SAFETY: the variable name is unique to this test and nothing else reads it.
        unsafe { std::env::set_var("DOCENT_TEST_CONFIG_DIR", "/tmp/docent-config") };
        let path = config_dir_from_env(Some(PathBuf::from("$DOCENT_TEST_CONFIG_DIR"))).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/docent-config"));
    }

    #[test]
    fn config_dir_rejects_an_undefined_variable() {
        let error = config_dir_from_env(Some(PathBuf::from("$DOCENT_TEST_MISSING_CONFIG_DIR")))
            .unwrap_err();
        assert!(matches!(error, DocentError::ExpandPath { .. }));
    }

    #[test]
    fn config_rejects_invalid_values() {
        let mut config = Config::default();
        config.provider.max_tokens = 0;
        config.provider.base_url = "ftp://example.com".to_string();
        let error = config.validate(Path::new("config.toml")).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("max_tokens"));
        assert!(message.contains("base_url"));
    }

    #[test]
    fn init_creates_skips_and_forces() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("docent");

        let report = init(&dir, false).unwrap();
        assert_eq!(report.created.len(), 2 + defaults::PROFILES.len());
        assert!(config_file(&dir).exists());
        assert!(dir.join(ANSWER_CSS_FILE).exists());
        for (name, _) in defaults::PROFILES {
            assert!(profiles_dir(&dir).join(format!("{name}.toml")).exists());
        }

        let report = init(&dir, false).unwrap();
        assert!(report.created.is_empty());
        assert_eq!(report.skipped.len(), 2 + defaults::PROFILES.len());

        fs::write(config_file(&dir), "garbage").unwrap();
        let report = init(&dir, true).unwrap();
        assert_eq!(report.created.len(), 2 + defaults::PROFILES.len());
        assert!(
            fs::read_to_string(config_file(&dir))
                .unwrap()
                .contains("[provider]")
        );
    }

    #[test]
    fn init_creates_an_answer_css_example() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("docent");

        let report = init(&dir, false).unwrap();

        let css_path = dir.join(ANSWER_CSS_FILE);
        assert!(report.created.contains(&css_path));
        let css = fs::read_to_string(&css_path).unwrap();
        assert!(css.contains("--docent-font-body"));
        assert!(css.contains("custom_css"));
    }

    #[test]
    fn init_skips_and_forces_answer_css() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("docent");
        init(&dir, false).unwrap();

        let css_path = dir.join(ANSWER_CSS_FILE);
        fs::write(&css_path, "/* custom */\n").unwrap();

        init(&dir, false).unwrap();
        assert_eq!(fs::read_to_string(&css_path).unwrap(), "/* custom */\n");

        init(&dir, true).unwrap();
        assert!(
            fs::read_to_string(&css_path)
                .unwrap()
                .contains("--docent-font-body")
        );
    }

    #[test]
    fn init_preserves_user_profiles() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("docent");
        init(&dir, false).unwrap();

        let custom = profiles_dir(&dir).join("custom.toml");
        fs::write(&custom, "user_prompt = \"hello\"\n").unwrap();

        init(&dir, true).unwrap();
        assert!(custom.exists());
        assert_eq!(
            fs::read_to_string(custom).unwrap(),
            "user_prompt = \"hello\"\n"
        );
    }

    #[test]
    fn init_rejects_a_file_as_config_dir() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("not-a-dir");
        fs::write(&path, "x").unwrap();

        let error = init(&path, false).unwrap_err();
        assert!(matches!(error, DocentError::ConfigDirNotDirectory(_)));
    }

    #[test]
    fn config_rejects_empty_and_out_of_range_values() {
        struct Case {
            expected: &'static str,
            mutate: fn(&mut Config),
        }

        let cases = [
            Case {
                expected: "provider.base_url must not be empty",
                mutate: |config| config.provider.base_url.clear(),
            },
            Case {
                expected: "provider.model must not be empty",
                mutate: |config| config.provider.model.clear(),
            },
            Case {
                expected: "provider.timeout_secs must be greater than zero",
                mutate: |config| config.provider.timeout_secs = 0,
            },
            Case {
                expected: "provider.temperature must be between 0.0 and 2.0",
                mutate: |config| config.provider.temperature = 2.5,
            },
            Case {
                expected: "context.max_chars must be greater than zero",
                mutate: |config| config.context.max_chars = 0,
            },
            Case {
                expected: "response.language must not be empty",
                mutate: |config| config.response.language.clear(),
            },
            Case {
                expected: "response.target_language must not be empty",
                mutate: |config| config.response.target_language.clear(),
            },
        ];

        for case in cases {
            let mut config = Config::default();
            (case.mutate)(&mut config);

            let error = config.validate(Path::new("config.toml")).unwrap_err();

            assert!(
                error.to_string().contains(case.expected),
                "missing {:?} in {error}",
                case.expected
            );
        }
    }
}
