//! Error types shared by the library and the binary.

use std::path::PathBuf;

/// Errors reported while loading configuration or profiles.
#[derive(Debug, thiserror::Error)]
pub enum DocentError {
    #[error("could not determine a config directory; set DOCENT_CONFIG_DIR or pass --config-dir")]
    NoConfigDir,

    #[error("config path {0} exists but is not a directory")]
    ConfigDirNotDirectory(PathBuf),

    #[error("failed to create directory {path}: {source}")]
    CreateDir {
        /// Directory that could not be created.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read {path}: {source}")]
    Read {
        /// File that could not be read.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write {path}: {source}")]
    Write {
        /// File that could not be written.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read directory {path}: {source}")]
    ReadDir {
        /// Directory that could not be read.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse config {path}: {source}")]
    ParseConfig {
        /// Config file being parsed.
        path: PathBuf,
        /// TOML parse error.
        #[source]
        source: toml::de::Error,
    },

    #[error("failed to parse profile {path}: {source}")]
    ParseProfile {
        /// Profile file being parsed.
        path: PathBuf,
        /// TOML parse error.
        #[source]
        source: toml::de::Error,
    },

    #[error("invalid config {path}: {message}")]
    InvalidConfig {
        /// Config file that failed validation.
        path: PathBuf,
        /// Human readable validation failures.
        message: String,
    },

    #[error("invalid profile {path}: {message}")]
    InvalidProfile {
        /// Profile file that failed validation.
        path: PathBuf,
        /// Human readable validation failures.
        message: String,
    },

    #[error("invalid profile name '{0}'; use letters, digits, '_' or '-'")]
    InvalidProfileName(String),

    #[error("profile '{name}' not found in {dir}")]
    ProfileNotFound {
        /// Requested profile name.
        name: String,
        /// Directory searched for profiles.
        dir: PathBuf,
    },

    #[error("no profiles found in {0}; run 'docent init'")]
    NoProfiles(PathBuf),

    #[error("no valid profiles found in {dir}: {messages}")]
    NoValidProfiles {
        /// Directory searched for profiles.
        dir: PathBuf,
        /// Validation failures for the profiles that were skipped.
        messages: String,
    },

    #[error("failed to serialize profiles: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("failed to read stdin: {0}")]
    ReadStdin(#[source] std::io::Error),

    #[error("profile requires {input} but it was not provided")]
    MissingInput {
        /// Missing input name.
        input: &'static str,
    },

    #[error("invalid settings: {0}")]
    InvalidSettings(String),

    #[error("missing API key: environment variable {env} is not set")]
    MissingApiKey {
        /// Environment variable that should hold the key.
        env: String,
    },

    #[error("provider request failed with HTTP {status}: {message}")]
    ProviderHttp {
        /// HTTP status code.
        status: u16,
        /// Provider error message or a truncated response body.
        message: String,
    },

    #[error("provider request failed: {message}")]
    ProviderTransport {
        /// Transport, timeout, or response parsing error.
        message: String,
    },

    #[error("provider returned an empty response")]
    EmptyResponse,
}

impl DocentError {
    /// Process exit code for this error.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::InvalidRequest(_) | Self::MissingInput { .. } | Self::ReadStdin(_) => 4,
            Self::MissingApiKey { .. } | Self::InvalidSettings(_) => 2,
            Self::ProviderHttp { .. } | Self::ProviderTransport { .. } | Self::EmptyResponse => 3,
            _ => 2,
        }
    }
}
