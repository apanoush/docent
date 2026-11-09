//! Local SQLite storage for provider runs and the answer cache.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use rusqlite::{Connection, params};

use crate::config::{Config, HistoryConfig};
use crate::error::DocentError;

/// Schema version understood by this build.
pub const SCHEMA_VERSION: i64 = 3;

/// Database filename used in the platform data directory.
pub const DATABASE_FILE: &str = "docent.db";

/// One model run to record.
#[derive(Debug, Clone, PartialEq)]
pub struct NewRun {
    /// Unix epoch milliseconds when the provider call started.
    pub created_at_ms: i64,
    /// Provider call duration in milliseconds.
    pub duration_ms: i64,
    /// Command that produced the run, for example `explain` or `request`.
    pub command: String,
    /// Profile name.
    pub profile: String,
    /// Origin of the request, for example `cli` or `nvim`.
    pub source: String,
    /// Output mode used by the command.
    pub output_mode: String,
    /// `success` or `error`.
    pub status: String,
    /// Error message when the run failed.
    pub error: Option<String>,
    /// File name used for context.
    pub file: Option<String>,
    /// Selected passage.
    pub selection: String,
    /// Follow-up question, when the profile used one.
    pub question: Option<String>,
    /// Size of the original document in bytes.
    pub document_bytes: Option<i64>,
    /// Selection start offset in the document.
    pub byte_start: Option<i64>,
    /// Selection end offset in the document.
    pub byte_end: Option<i64>,
    /// Effective context mode.
    pub context_mode: String,
    /// Configured character budget for `auto` context.
    pub context_max_chars: i64,
    /// Character count of the assembled context.
    pub context_chars: i64,
    /// Whether the context omitted part of the document.
    pub context_truncated: bool,
    /// Provider wire format.
    pub wire_format: String,
    /// Provider base URL.
    pub base_url: String,
    /// Model reported by the provider, or the configured model on failure.
    pub model: String,
    /// Sampling temperature.
    pub temperature: f64,
    /// Maximum output tokens.
    pub max_tokens: i64,
    /// Provider timeout in seconds.
    pub timeout_secs: i64,
    /// Effective response language.
    pub response_language: String,
    /// Effective translation target language.
    pub target_language: String,
    /// Rendered system message.
    pub system_prompt: String,
    /// Rendered user message.
    pub user_prompt: String,
    /// Full answer on success, partial answer on a failed stream.
    pub output: String,
    /// Input tokens reported by the provider.
    pub input_tokens: Option<i64>,
    /// Output tokens reported by the provider.
    pub output_tokens: Option<i64>,
    /// Docent version that produced the run.
    pub docent_version: String,
}

/// Returns the current time as Unix epoch milliseconds.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// Resolves the history database path.
///
/// An explicit `history.database` wins. Its `~` and environment variables are
/// expanded shell-style, and relative paths are resolved against the config
/// directory. Otherwise the platform data directory is used.
pub fn database_path(
    config_dir: &Path,
    history_config: &HistoryConfig,
) -> Result<PathBuf, DocentError> {
    if let Some(path) = &history_config.database {
        let path = crate::config::expand_path(path)?;
        return Ok(if path.is_absolute() {
            path
        } else {
            config_dir.join(path)
        });
    }

    default_database_path()
}

/// Returns the default database path in the platform data directory.
pub fn default_database_path() -> Result<PathBuf, DocentError> {
    ProjectDirs::from("", "", "docent")
        .map(|dirs| dirs.data_dir().join(DATABASE_FILE))
        .ok_or(DocentError::NoDataDir)
}

/// Records one run when history is enabled.
pub fn write(config: &Config, config_dir: &Path, run: &NewRun) -> Result<(), DocentError> {
    if !config.history.enabled {
        return Ok(());
    }

    let path = database_path(config_dir, &config.history)?;
    let connection = open(&path)?;
    insert(&connection, &path, run)?;
    restrict_permissions(&path);
    Ok(())
}

/// Opens the history database, applying the cache table migration when needed.
///
/// Callers that read from another table also use this, so schema changes stay in
/// one place.
pub(crate) fn open_database(
    config: &Config,
    config_dir: &Path,
) -> Result<(Connection, PathBuf), DocentError> {
    let path = database_path(config_dir, &config.history)?;
    let connection = open(&path)?;
    Ok((connection, path))
}

fn open(path: &Path) -> Result<Connection, DocentError> {
    create_parent(path)?;
    let connection = Connection::open(path).map_err(|source| DocentError::History {
        path: path.to_path_buf(),
        source,
    })?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|source| DocentError::History {
            path: path.to_path_buf(),
            source,
        })?;
    connection
        .execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
        .map_err(|source| DocentError::History {
            path: path.to_path_buf(),
            source,
        })?;
    ensure_schema(&connection, path)?;
    restrict_permissions(path);
    Ok(connection)
}

fn ensure_schema(connection: &Connection, path: &Path) -> Result<(), DocentError> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|source| DocentError::History {
            path: path.to_path_buf(),
            source,
        })?;

    match version {
        0 => {
            connection
                .execute_batch(SCHEMA_V1)
                .map_err(|source| DocentError::History {
                    path: path.to_path_buf(),
                    source,
                })?;
            connection
                .execute_batch(SCHEMA_V2)
                .map_err(|source| DocentError::History {
                    path: path.to_path_buf(),
                    source,
                })?;
            connection
                .execute_batch(SCHEMA_V3)
                .map_err(|source| DocentError::History {
                    path: path.to_path_buf(),
                    source,
                })?;
            set_schema_version(connection, path)
        }
        1 => {
            connection
                .execute_batch(SCHEMA_V2)
                .map_err(|source| DocentError::History {
                    path: path.to_path_buf(),
                    source,
                })?;
            connection
                .execute_batch(SCHEMA_V3)
                .map_err(|source| DocentError::History {
                    path: path.to_path_buf(),
                    source,
                })?;
            set_schema_version(connection, path)
        }
        2 => {
            connection
                .execute_batch(SCHEMA_V3)
                .map_err(|source| DocentError::History {
                    path: path.to_path_buf(),
                    source,
                })?;
            set_schema_version(connection, path)
        }
        SCHEMA_VERSION => Ok(()),
        version => Err(DocentError::HistorySchema {
            path: path.to_path_buf(),
            version,
            supported: SCHEMA_VERSION,
        }),
    }
}

fn set_schema_version(connection: &Connection, path: &Path) -> Result<(), DocentError> {
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|source| DocentError::History {
            path: path.to_path_buf(),
            source,
        })
}

fn insert(connection: &Connection, path: &Path, run: &NewRun) -> Result<(), DocentError> {
    connection
        .execute(
            INSERT_RUN,
            params![
                run.created_at_ms,
                run.duration_ms,
                run.command,
                run.profile,
                run.source,
                run.output_mode,
                run.status,
                run.error,
                run.file,
                run.selection,
                run.question,
                run.document_bytes,
                run.byte_start,
                run.byte_end,
                run.context_mode,
                run.context_max_chars,
                run.context_chars,
                i64::from(run.context_truncated),
                run.wire_format,
                run.base_url,
                run.model,
                run.temperature,
                run.max_tokens,
                run.timeout_secs,
                run.response_language,
                run.target_language,
                run.system_prompt,
                run.user_prompt,
                run.output,
                run.input_tokens,
                run.output_tokens,
                run.docent_version,
            ],
        )
        .map(|_| ())
        .map_err(|source| DocentError::History {
            path: path.to_path_buf(),
            source,
        })
}

fn create_parent(path: &Path) -> Result<(), DocentError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }

    let existed = parent.exists();
    fs::create_dir_all(parent).map_err(|source| DocentError::CreateDir {
        path: parent.to_path_buf(),
        source,
    })?;

    #[cfg(unix)]
    if !existed {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
    }

    Ok(())
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        let sidecar = PathBuf::from(sidecar);
        if sidecar.exists() {
            let _ = fs::set_permissions(sidecar, fs::Permissions::from_mode(0o600));
        }
    }
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    command TEXT NOT NULL,
    profile TEXT NOT NULL,
    source TEXT NOT NULL,
    output_mode TEXT NOT NULL,
    status TEXT NOT NULL,
    error TEXT,
    file TEXT,
    selection TEXT NOT NULL,
    question TEXT,
    document_bytes INTEGER,
    byte_start INTEGER,
    byte_end INTEGER,
    context_mode TEXT NOT NULL,
    context_max_chars INTEGER NOT NULL,
    context_chars INTEGER NOT NULL,
    context_truncated INTEGER NOT NULL,
    provider TEXT NOT NULL,
    base_url TEXT NOT NULL,
    model TEXT NOT NULL,
    temperature REAL NOT NULL,
    max_tokens INTEGER NOT NULL,
    timeout_secs INTEGER NOT NULL,
    response_language TEXT NOT NULL,
    target_language TEXT NOT NULL,
    system_prompt TEXT NOT NULL,
    user_prompt TEXT NOT NULL,
    output TEXT NOT NULL DEFAULT '',
    input_tokens INTEGER,
    output_tokens INTEGER,
    docent_version TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_runs_created_at ON runs(created_at);
CREATE INDEX IF NOT EXISTS idx_runs_profile ON runs(profile);
CREATE INDEX IF NOT EXISTS idx_runs_source ON runs(source);
"#;

const SCHEMA_V2: &str = r#"
CREATE TABLE IF NOT EXISTS cache (
    key BLOB PRIMARY KEY,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER NOT NULL,
    hits INTEGER NOT NULL DEFAULT 0,
    model TEXT,
    output TEXT NOT NULL,
    docent_version TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_cache_last_used_at ON cache(last_used_at);
"#;

const SCHEMA_V3: &str = r#"
ALTER TABLE runs RENAME COLUMN provider TO wire_format;
"#;

const INSERT_RUN: &str = r#"
INSERT INTO runs (
    created_at,
    duration_ms,
    command,
    profile,
    source,
    output_mode,
    status,
    error,
    file,
    selection,
    question,
    document_bytes,
    byte_start,
    byte_end,
    context_mode,
    context_max_chars,
    context_chars,
    context_truncated,
    wire_format,
    base_url,
    model,
    temperature,
    max_tokens,
    timeout_secs,
    response_language,
    target_language,
    system_prompt,
    user_prompt,
    output,
    input_tokens,
    output_tokens,
    docent_version
) VALUES (
    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
    ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30,
    ?31, ?32
)
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use directories::BaseDirs;

    fn sample_run() -> NewRun {
        NewRun {
            created_at_ms: 1_700_000_000_000,
            duration_ms: 42,
            command: "explain".to_string(),
            profile: "explain".to_string(),
            source: "cli".to_string(),
            output_mode: "plain_stream".to_string(),
            status: "success".to_string(),
            error: None,
            file: Some("notes.md".to_string()),
            selection: "beta".to_string(),
            question: None,
            document_bytes: Some(16),
            byte_start: Some(6),
            byte_end: Some(10),
            context_mode: "auto".to_string(),
            context_max_chars: 32_000,
            context_chars: 80,
            context_truncated: false,
            wire_format: "openai".to_string(),
            base_url: "https://example.com/v1".to_string(),
            model: "test-model".to_string(),
            temperature: 0.3,
            max_tokens: 1200,
            timeout_secs: 120,
            response_language: "auto".to_string(),
            target_language: "English".to_string(),
            system_prompt: "You are Docent.".to_string(),
            user_prompt: "Document: alpha beta gamma".to_string(),
            output: "An answer".to_string(),
            input_tokens: Some(3),
            output_tokens: Some(4),
            docent_version: "0.1.0".to_string(),
        }
    }

    fn config_with_database(path: &Path) -> Config {
        let mut config = Config::default();
        config.history.database = Some(path.to_path_buf());
        config
    }

    #[test]
    fn explicit_database_path_wins() {
        let history = HistoryConfig {
            enabled: true,
            database: Some(PathBuf::from("/tmp/history.db")),
        };
        let path = database_path(Path::new("/tmp/config"), &history).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/history.db"));
    }

    #[test]
    fn relative_database_path_uses_the_config_directory() {
        let history = HistoryConfig {
            enabled: true,
            database: Some(PathBuf::from("history.db")),
        };
        let path = database_path(Path::new("/tmp/config"), &history).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/config/history.db"));
    }

    #[test]
    fn tilde_is_expanded() {
        let Some(dirs) = BaseDirs::new() else {
            return;
        };
        let history = HistoryConfig {
            enabled: true,
            database: Some(PathBuf::from("~/history.db")),
        };
        let path = database_path(Path::new("/tmp/config"), &history).unwrap();
        assert_eq!(path, dirs.home_dir().join("history.db"));
    }

    #[test]
    fn environment_variables_are_expanded() {
        // SAFETY: the variable name is unique to this test and nothing else reads it.
        unsafe { std::env::set_var("DOCENT_TEST_HISTORY_DIR", "/tmp/docent-history") };
        let history = HistoryConfig {
            enabled: true,
            database: Some(PathBuf::from("$DOCENT_TEST_HISTORY_DIR/history.db")),
        };

        let path = database_path(Path::new("/tmp/config"), &history).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/docent-history/history.db"));
    }

    #[test]
    fn undefined_environment_variables_are_rejected() {
        let history = HistoryConfig {
            enabled: true,
            database: Some(PathBuf::from("$DOCENT_TEST_MISSING_HISTORY_DIR/history.db")),
        };

        let error = database_path(Path::new("/tmp/config"), &history).unwrap_err();
        assert!(matches!(error, DocentError::ExpandPath { .. }));
    }

    #[test]
    fn default_path_lives_in_the_docent_data_directory() {
        let path = default_database_path().unwrap();
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(DATABASE_FILE)
        );
        assert_eq!(
            path.parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str()),
            Some("docent")
        );
    }

    #[test]
    fn writes_and_reads_a_successful_run() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("history.db");
        let config = config_with_database(&database);

        write(&config, temp.path(), &sample_run()).unwrap();

        let connection = open(&database).unwrap();
        let row: (String, String, String, String, String, i64, i64) = connection
            .query_row(
                "SELECT command, profile, source, model, output, input_tokens, output_tokens FROM runs",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(
            row,
            (
                "explain".to_string(),
                "explain".to_string(),
                "cli".to_string(),
                "test-model".to_string(),
                "An answer".to_string(),
                3,
                4,
            )
        );
    }

    #[test]
    fn stores_errors_and_partial_output() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("history.db");
        let config = config_with_database(&database);
        let mut run = sample_run();
        run.status = "error".to_string();
        run.error = Some("provider request failed".to_string());
        run.output = "partial".to_string();
        run.input_tokens = None;
        run.output_tokens = None;

        write(&config, temp.path(), &run).unwrap();

        let connection = open(&database).unwrap();
        let row: (String, String, String) = connection
            .query_row("SELECT status, error, output FROM runs", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(
            row,
            (
                "error".to_string(),
                "provider request failed".to_string(),
                "partial".to_string(),
            )
        );
    }

    #[test]
    fn disabled_history_does_not_create_a_file() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("history.db");
        let mut config = config_with_database(&database);
        config.history.enabled = false;

        write(&config, temp.path(), &sample_run()).unwrap();
        assert!(!database.exists());
    }

    #[test]
    fn newer_schema_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("history.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        drop(connection);

        let config = config_with_database(&database);
        let error = write(&config, temp.path(), &sample_run()).unwrap_err();
        assert!(error.to_string().contains("schema version"));
    }

    #[cfg(unix)]
    #[test]
    fn database_file_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("history.db");
        let config = config_with_database(&database);

        write(&config, temp.path(), &sample_run()).unwrap();

        let mode = fs::metadata(&database).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn schema_has_a_version() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("history.db");
        let config = config_with_database(&database);
        write(&config, temp.path(), &sample_run()).unwrap();

        let connection = open(&database).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    fn table_exists(connection: &Connection, name: &str) -> bool {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [name],
                |row| row.get(0),
            )
            .unwrap();
        count == 1
    }

    fn column_exists(connection: &Connection, table: &str, column: &str) -> bool {
        let count: i64 = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = ?1"),
                [column],
                |row| row.get(0),
            )
            .unwrap();
        count == 1
    }

    /// Inserts one row using the pre-version-3 `provider` column.
    fn insert_legacy_run(connection: &Connection, path: &Path) -> Result<(), DocentError> {
        connection
            .execute(
                "INSERT INTO runs (
                    created_at, duration_ms, command, profile, source, output_mode, status,
                    selection, context_mode, context_max_chars, context_chars,
                    context_truncated, provider, base_url, model, temperature,
                    max_tokens, timeout_secs, response_language, target_language,
                    system_prompt, user_prompt, output, docent_version
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                    ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24
                )",
                params![
                    1_700_000_000_000i64,
                    42i64,
                    "explain",
                    "explain",
                    "cli",
                    "plain_stream",
                    "success",
                    "beta",
                    "auto",
                    32_000i64,
                    80i64,
                    0i64,
                    "openai",
                    "https://example.com/v1",
                    "test-model",
                    0.3f64,
                    1200i64,
                    120i64,
                    "auto",
                    "English",
                    "You are Docent.",
                    "Document: alpha beta gamma",
                    "An answer",
                    "0.1.0",
                ],
            )
            .map(|_| ())
            .map_err(|source| DocentError::History {
                path: path.to_path_buf(),
                source,
            })
    }

    #[test]
    fn fresh_database_has_the_cache_table() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("history.db");
        let config = config_with_database(&database);

        write(&config, temp.path(), &sample_run()).unwrap();

        let connection = open(&database).unwrap();
        assert!(table_exists(&connection, "runs"));
        assert!(table_exists(&connection, "cache"));
    }

    #[test]
    fn migrates_a_version_1_database_and_keeps_runs() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("history.db");

        let connection = Connection::open(&database).unwrap();
        connection.execute_batch(SCHEMA_V1).unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        insert_legacy_run(&connection, &database).unwrap();
        drop(connection);

        let config = config_with_database(&database);
        write(&config, temp.path(), &sample_run()).unwrap();

        let connection = open(&database).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
        assert!(table_exists(&connection, "cache"));
        assert!(column_exists(&connection, "runs", "wire_format"));
        assert!(!column_exists(&connection, "runs", "provider"));
        let runs: i64 = connection
            .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(runs, 2);
    }

    #[test]
    fn migrates_a_version_2_database_and_renames_provider_to_wire_format() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("history.db");

        let connection = Connection::open(&database).unwrap();
        connection.execute_batch(SCHEMA_V1).unwrap();
        connection.execute_batch(SCHEMA_V2).unwrap();
        connection.pragma_update(None, "user_version", 2).unwrap();
        insert_legacy_run(&connection, &database).unwrap();
        drop(connection);

        let connection = open(&database).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
        assert!(column_exists(&connection, "runs", "wire_format"));
        assert!(!column_exists(&connection, "runs", "provider"));
        let runs: i64 = connection
            .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(runs, 1);
    }
}
