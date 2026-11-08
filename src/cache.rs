//! Exact-match answer cache backed by the history SQLite database.

use std::path::Path;

use rusqlite::params;

use crate::config::Config;
use crate::error::DocentError;
use crate::history;
use crate::prompt::{EffectiveSettings, Prompt};

/// Version tag mixed into every cache key.
///
/// Bump this when the key recipe changes so older entries stop matching.
pub const KEY_VERSION: u8 = 1;

/// An answer loaded from the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedAnswer {
    /// Stored answer text.
    pub output: String,
    /// Model reported by the provider when the answer was stored.
    pub model: Option<String>,
}

/// Computes the cache key for a rendered prompt and its effective settings.
///
/// The rendered prompt already contains the selection, the context window, the
/// question, the file label, and the languages, so only the provider parameters
/// that change the answer are added here.
pub fn key(config: &Config, settings: &EffectiveSettings, prompt: &Prompt) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[KEY_VERSION]);
    feed(&mut hasher, config.api.kind.as_str().as_bytes());
    feed(&mut hasher, config.api.base_url.as_bytes());
    feed(&mut hasher, settings.model.as_bytes());
    hasher.update(&settings.temperature.to_bits().to_le_bytes());
    hasher.update(&settings.max_tokens.to_le_bytes());
    feed(&mut hasher, prompt.system.as_bytes());
    feed(&mut hasher, prompt.user.as_bytes());
    *hasher.finalize().as_bytes()
}

/// Mixes a length prefix in so concatenated values cannot collide.
fn feed(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

/// Returns true when both the cache and history storage are enabled.
///
/// The cache lives in the history database, so answers are not stored when the
/// user asked not to keep history.
pub fn enabled(config: &Config) -> bool {
    config.cache.enabled && config.history.enabled
}

/// Looks up a stored answer and records the use.
pub fn lookup(
    config: &Config,
    config_dir: &Path,
    key: &[u8; 32],
) -> Result<Option<CachedAnswer>, DocentError> {
    if !enabled(config) {
        return Ok(None);
    }

    let (connection, path) = history::open_database(config, config_dir)?;
    let row = connection.query_row(SELECT_CACHE, params![key.as_slice()], |row| {
        Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?))
    });

    let answer = match row {
        Ok((model, output)) => CachedAnswer { output, model },
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(source) => return Err(DocentError::History { path, source }),
    };

    // A failed statistics update must not hide the answer.
    if let Err(error) = connection.execute(TOUCH_CACHE, params![history::now_ms(), key.as_slice()])
    {
        log::warn!("could not update cache statistics: {error}");
    }

    Ok(Some(answer))
}

/// Stores a successful answer and evicts the least recently used entries.
pub fn store(
    config: &Config,
    config_dir: &Path,
    key: &[u8; 32],
    model: Option<&str>,
    output: &str,
) -> Result<(), DocentError> {
    if !enabled(config) || output.is_empty() {
        return Ok(());
    }

    let (mut connection, path) = history::open_database(config, config_dir)?;
    let now = history::now_ms();
    let max_entries = config.cache.max_entries.min(i64::MAX as usize) as i64;

    let transaction = connection
        .transaction()
        .map_err(|source| DocentError::History {
            path: path.clone(),
            source,
        })?;
    transaction
        .execute(
            UPSERT_CACHE,
            params![
                key.as_slice(),
                now,
                now,
                model,
                output,
                env!("CARGO_PKG_VERSION")
            ],
        )
        .map_err(|source| DocentError::History {
            path: path.clone(),
            source,
        })?;
    transaction
        .execute(EVICT_CACHE, params![max_entries])
        .map_err(|source| DocentError::History {
            path: path.clone(),
            source,
        })?;
    transaction
        .commit()
        .map_err(|source| DocentError::History { path, source })?;

    Ok(())
}

const SELECT_CACHE: &str = "SELECT model, output FROM cache WHERE key = ?1";

const TOUCH_CACHE: &str = "UPDATE cache SET hits = hits + 1, last_used_at = ?1 WHERE key = ?2";

const UPSERT_CACHE: &str = r#"
INSERT INTO cache (key, created_at, last_used_at, hits, model, output, docent_version)
VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6)
ON CONFLICT(key) DO UPDATE SET
    created_at = excluded.created_at,
    last_used_at = excluded.last_used_at,
    model = excluded.model,
    output = excluded.output,
    docent_version = excluded.docent_version
"#;

const EVICT_CACHE: &str = r#"
DELETE FROM cache
WHERE rowid NOT IN (
    SELECT rowid FROM cache ORDER BY last_used_at DESC, rowid DESC LIMIT ?1
)
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiKind, ContextMode};
    use crate::prompt::EffectiveSettings;

    fn config(directory: &Path) -> Config {
        let mut config = Config::default();
        config.history.database = Some(directory.join("docent.db"));
        config
    }

    fn settings() -> EffectiveSettings {
        EffectiveSettings {
            model: "test-model".to_string(),
            temperature: 0.3,
            max_tokens: 1200,
            context_mode: ContextMode::Auto,
            response_language: "auto".to_string(),
            target_language: "English".to_string(),
        }
    }

    fn prompt() -> Prompt {
        Prompt {
            system: "You are Docent.".to_string(),
            user: "<document>alpha beta gamma</document>".to_string(),
        }
    }

    #[test]
    fn key_is_stable_for_the_same_inputs() {
        let directory = tempfile::tempdir().unwrap();
        let config = config(directory.path());
        assert_eq!(
            key(&config, &settings(), &prompt()),
            key(&config, &settings(), &prompt())
        );
    }

    #[test]
    fn key_changes_with_every_parameter() {
        let directory = tempfile::tempdir().unwrap();
        let base_config = config(directory.path());
        let base = key(&base_config, &settings(), &prompt());

        let mut other_config = base_config.clone();
        other_config.api.kind = ApiKind::Anthropic;
        assert_ne!(base, key(&other_config, &settings(), &prompt()));

        let mut other_config = base_config.clone();
        other_config.api.base_url = "https://example.com/other".to_string();
        assert_ne!(base, key(&other_config, &settings(), &prompt()));

        let mut other_settings = settings();
        other_settings.model = "other-model".to_string();
        assert_ne!(base, key(&base_config, &other_settings, &prompt()));

        let mut other_settings = settings();
        other_settings.temperature = 0.4;
        assert_ne!(base, key(&base_config, &other_settings, &prompt()));

        let mut other_settings = settings();
        other_settings.max_tokens = 900;
        assert_ne!(base, key(&base_config, &other_settings, &prompt()));

        let mut other_prompt = prompt();
        other_prompt.system = "Other system.".to_string();
        assert_ne!(base, key(&base_config, &settings(), &other_prompt));

        let mut other_prompt = prompt();
        other_prompt.user = "other user".to_string();
        assert_ne!(base, key(&base_config, &settings(), &other_prompt));
    }

    #[test]
    fn length_prefixes_do_not_collide() {
        let directory = tempfile::tempdir().unwrap();
        let config = config(directory.path());
        let mut left = prompt();
        let mut right = prompt();
        left.system = "ab".to_string();
        left.user = "c".to_string();
        right.system = "a".to_string();
        right.user = "bc".to_string();
        assert_ne!(
            key(&config, &settings(), &left),
            key(&config, &settings(), &right)
        );
    }

    #[test]
    fn stored_answers_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let config = config(directory.path());
        let cache_key = key(&config, &settings(), &prompt());

        store(
            &config,
            directory.path(),
            &cache_key,
            Some("test-model"),
            "An answer",
        )
        .unwrap();
        let answer = lookup(&config, directory.path(), &cache_key).unwrap();

        assert_eq!(
            answer,
            Some(CachedAnswer {
                output: "An answer".to_string(),
                model: Some("test-model".to_string()),
            })
        );
    }

    #[test]
    fn missing_entries_return_none() {
        let directory = tempfile::tempdir().unwrap();
        let config = config(directory.path());
        let cache_key = key(&config, &settings(), &prompt());

        assert_eq!(lookup(&config, directory.path(), &cache_key).unwrap(), None);
    }

    #[test]
    fn empty_output_is_not_stored() {
        let directory = tempfile::tempdir().unwrap();
        let config = config(directory.path());
        let cache_key = key(&config, &settings(), &prompt());

        store(&config, directory.path(), &cache_key, None, "").unwrap();

        assert_eq!(lookup(&config, directory.path(), &cache_key).unwrap(), None);
    }

    #[test]
    fn lookup_increments_hits_and_updates_last_used_at() {
        let directory = tempfile::tempdir().unwrap();
        let config = config(directory.path());
        let cache_key = key(&config, &settings(), &prompt());
        store(&config, directory.path(), &cache_key, None, "An answer").unwrap();

        lookup(&config, directory.path(), &cache_key).unwrap();
        lookup(&config, directory.path(), &cache_key).unwrap();

        let connection = rusqlite::Connection::open(directory.path().join("docent.db")).unwrap();
        let (hits, last_used): (i64, i64) = connection
            .query_row("SELECT hits, last_used_at FROM cache", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(hits, 2);
        assert!(last_used > 0);
    }

    #[test]
    fn storing_the_same_key_replaces_the_answer_and_keeps_hits() {
        let directory = tempfile::tempdir().unwrap();
        let config = config(directory.path());
        let cache_key = key(&config, &settings(), &prompt());

        store(&config, directory.path(), &cache_key, None, "first").unwrap();
        lookup(&config, directory.path(), &cache_key).unwrap();
        store(&config, directory.path(), &cache_key, None, "second").unwrap();

        let answer = lookup(&config, directory.path(), &cache_key)
            .unwrap()
            .unwrap();
        assert_eq!(answer.output, "second");

        let connection = rusqlite::Connection::open(directory.path().join("docent.db")).unwrap();
        let hits: i64 = connection
            .query_row("SELECT hits FROM cache", [], |row| row.get(0))
            .unwrap();
        assert_eq!(hits, 2);
    }

    #[test]
    fn old_entries_are_evicted() {
        let directory = tempfile::tempdir().unwrap();
        let mut config = config(directory.path());
        config.cache.max_entries = 2;

        for index in 0..3 {
            let mut prompt = prompt();
            prompt.user = format!("document {index}");
            let cache_key = key(&config, &settings(), &prompt);
            store(
                &config,
                directory.path(),
                &cache_key,
                None,
                &format!("answer {index}"),
            )
            .unwrap();
        }

        let connection = rusqlite::Connection::open(directory.path().join("docent.db")).unwrap();
        let outputs: Vec<String> = connection
            .prepare("SELECT output FROM cache ORDER BY rowid")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(outputs, vec!["answer 1", "answer 2"]);
    }

    #[test]
    fn disabled_cache_does_not_create_a_database() {
        let directory = tempfile::tempdir().unwrap();
        let mut config = config(directory.path());
        config.cache.enabled = false;
        let cache_key = key(&config, &settings(), &prompt());

        store(&config, directory.path(), &cache_key, None, "answer").unwrap();

        assert_eq!(lookup(&config, directory.path(), &cache_key).unwrap(), None);
        assert!(!directory.path().join("docent.db").exists());
    }

    #[test]
    fn disabled_history_disables_the_cache() {
        let directory = tempfile::tempdir().unwrap();
        let mut config = config(directory.path());
        config.history.enabled = false;
        let cache_key = key(&config, &settings(), &prompt());

        store(&config, directory.path(), &cache_key, None, "answer").unwrap();

        assert_eq!(lookup(&config, directory.path(), &cache_key).unwrap(), None);
    }
}
