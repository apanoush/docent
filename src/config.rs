//! Configuration loading, validation, and default file creation.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::defaults;
use crate::error::DocentError;

/// Environment variable that overrides the config directory.
pub const CONFIG_DIR_ENV: &str = "DOCENT_CONFIG_DIR";

const CONFIG_FILE: &str = "config.toml";
const PROFILES_DIR: &str = "profiles";

/// Top level `config.toml` structure.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Provider settings.
    #[serde(default)]
    pub api: ApiConfig,
    /// Context window settings.
    #[serde(default)]
    pub context: ContextConfig,
    /// Response language settings.
    #[serde(default)]
    pub response: ResponseConfig,
}

/// Wire format used to talk to the provider.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApiKind {
    /// OpenAI Chat Completions, also used by OpenAI-compatible gateways.
    #[default]
    OpenAi,
    /// Anthropic Messages API.
    Anthropic,
}

/// Provider settings.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ApiConfig {
    /// Wire format.
    pub kind: ApiKind,
    /// Base URL, without the endpoint path.
    pub base_url: String,
    /// Model identifier.
    pub model: String,
    /// Environment variable holding the API key.
    pub api_key_env: String,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Maximum number of output tokens.
    pub max_tokens: u32,
    /// Sampling temperature.
    pub temperature: f32,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            kind: ApiKind::OpenAi,
            base_url: "https://opencode.ai/inference/go/openai/v1".to_string(),
            model: "deepseek-v4.1-flash".to_string(),
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

impl Config {
    /// Checks values that TOML parsing cannot enforce.
    pub fn validate(&self, path: &Path) -> Result<(), DocentError> {
        let mut errors = Vec::new();

        if self.api.base_url.trim().is_empty() {
            errors.push("api.base_url must not be empty".to_string());
        } else if !self.api.base_url.starts_with("http://")
            && !self.api.base_url.starts_with("https://")
        {
            errors.push("api.base_url must start with http:// or https://".to_string());
        }
        if self.api.model.trim().is_empty() {
            errors.push("api.model must not be empty".to_string());
        }
        if self.api.api_key_env.trim().is_empty() {
            errors.push("api.api_key_env must not be empty".to_string());
        }
        if self.api.timeout_secs == 0 {
            errors.push("api.timeout_secs must be greater than zero".to_string());
        }
        if self.api.max_tokens == 0 {
            errors.push("api.max_tokens must be greater than zero".to_string());
        }
        if !(0.0..=2.0).contains(&self.api.temperature) {
            errors.push(format!(
                "api.temperature must be between 0.0 and 2.0, got {}",
                self.api.temperature
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
pub fn config_dir_from_env(explicit: Option<PathBuf>) -> Result<PathBuf, DocentError> {
    resolve_config_dir(explicit, std::env::var_os(CONFIG_DIR_ENV))
}

/// Path to `config.toml`.
pub fn config_file(config_dir: &Path) -> PathBuf {
    config_dir.join(CONFIG_FILE)
}

/// Path to the profiles directory.
pub fn profiles_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(PROFILES_DIR)
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
    Ok(config)
}

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

/// Creates the config directory, `config.toml`, and the default profiles.
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

    write_default(
        &config_file(config_dir),
        defaults::CONFIG,
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

        assert_eq!(config.api.kind, ApiKind::OpenAi);
        assert_eq!(config.context.mode, ContextMode::Auto);
        assert_eq!(config.api.model, "deepseek-v4.1-flash");
    }

    #[test]
    fn unknown_config_fields_are_rejected() {
        let error = toml::from_str::<Config>("[api]\nunknown = 1\n").unwrap_err();
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
    fn config_rejects_invalid_values() {
        let mut config = Config::default();
        config.api.max_tokens = 0;
        config.api.base_url = "ftp://example.com".to_string();
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
        assert_eq!(report.created.len(), 1 + defaults::PROFILES.len());
        assert!(config_file(&dir).exists());
        for (name, _) in defaults::PROFILES {
            assert!(profiles_dir(&dir).join(format!("{name}.toml")).exists());
        }

        let report = init(&dir, false).unwrap();
        assert!(report.created.is_empty());
        assert_eq!(report.skipped.len(), 1 + defaults::PROFILES.len());

        fs::write(config_file(&dir), "garbage").unwrap();
        let report = init(&dir, true).unwrap();
        assert_eq!(report.created.len(), 1 + defaults::PROFILES.len());
        assert!(
            fs::read_to_string(config_file(&dir))
                .unwrap()
                .contains("[api]")
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
}
