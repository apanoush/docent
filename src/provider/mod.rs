//! Provider clients for OpenAI-compatible and Anthropic APIs.

pub mod anthropic;
pub mod openai;
pub mod stream;

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use serde::Serialize;
use serde_json::Value;

use crate::config::{ApiConfig, ApiKind, Config};
use crate::error::DocentError;
use crate::prompt::{EffectiveSettings, Prompt};

type HttpResponse = ureq::http::Response<ureq::Body>;

/// Token usage reported by a provider.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Usage {
    /// Input tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

impl Usage {
    pub(crate) fn new(input_tokens: Option<u64>, output_tokens: Option<u64>) -> Option<Self> {
        if input_tokens.is_none() && output_tokens.is_none() {
            None
        } else {
            Some(Self {
                input_tokens,
                output_tokens,
            })
        }
    }
}

/// Provider response.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Completion {
    /// Generated text.
    pub text: String,
    /// Model reported by the provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Token usage, when reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// True when the answer came from the local cache instead of the provider.
    #[serde(default, skip_serializing_if = "is_false")]
    pub cached: bool,
}

/// Returns true for the default `false`, so serialized output stays unchanged.
fn is_false(value: &bool) -> bool {
    !*value
}

/// One line of the NDJSON stream written by `docent run --stream`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// A text fragment produced by the model.
    Delta {
        /// Fragment text.
        text: String,
    },
    /// The complete answer rendered as a safe HTML fragment.
    ///
    /// Only emitted by `docent run --stream --html`, after the last delta and
    /// before the `done` event.
    Html {
        /// Rendered answer body.
        html: String,
    },
    /// The stream completed successfully.
    Done {
        /// Model reported by the provider.
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Token usage, when reported.
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        /// True when the answer came from the local cache.
        #[serde(default, skip_serializing_if = "is_false")]
        cached: bool,
    },
    /// The stream failed.
    Error {
        /// Human readable error message.
        message: String,
    },
}

pub(crate) struct ChatInput<'a> {
    pub system: &'a str,
    pub user: &'a str,
    pub model: &'a str,
    pub temperature: f32,
    pub max_tokens: u32,
}

/// Calls the configured provider and returns the completion.
pub fn complete(
    config: &Config,
    settings: &EffectiveSettings,
    prompt: &Prompt,
) -> Result<Completion, DocentError> {
    let api_key = api_key(config)?;
    let input = ChatInput {
        system: &prompt.system,
        user: &prompt.user,
        model: &settings.model,
        temperature: settings.temperature,
        max_tokens: settings.max_tokens,
    };

    match config.api.kind {
        ApiKind::OpenAi => openai::complete(config, &input, &api_key),
        ApiKind::Anthropic => anthropic::complete(config, &input, &api_key),
    }
}

/// Calls the configured provider and forwards text fragments as they arrive.
pub fn complete_streaming(
    config: &Config,
    settings: &EffectiveSettings,
    prompt: &Prompt,
    on_delta: &mut dyn FnMut(&str) -> Result<(), DocentError>,
) -> Result<Completion, DocentError> {
    let api_key = api_key(config)?;
    let input = ChatInput {
        system: &prompt.system,
        user: &prompt.user,
        model: &settings.model,
        temperature: settings.temperature,
        max_tokens: settings.max_tokens,
    };

    match config.api.kind {
        ApiKind::OpenAi => openai::complete_streaming(config, &input, &api_key, on_delta),
        ApiKind::Anthropic => anthropic::complete_streaming(config, &input, &api_key, on_delta),
    }
}

fn api_key(config: &Config) -> Result<String, DocentError> {
    let env_value = if config.api.api_key_env.trim().is_empty() {
        None
    } else {
        std::env::var(&config.api.api_key_env).ok()
    };
    resolve_api_key(&config.api, env_value)
}

/// Picks the API key. A non-empty environment value wins over a literal key.
fn resolve_api_key(api: &ApiConfig, env_value: Option<String>) -> Result<String, DocentError> {
    if let Some(value) = env_value.filter(|value| !value.trim().is_empty()) {
        return Ok(value);
    }
    if let Some(value) = api
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(value.to_string());
    }

    let detail = if api.api_key_env.trim().is_empty() {
        "set api.api_key in config.toml".to_string()
    } else {
        format!(
            "set api.api_key in config.toml or environment variable {}",
            api.api_key_env
        )
    };
    Err(DocentError::MissingApiKey { detail })
}

fn post_json(
    config: &Config,
    url: &str,
    headers: &[(&str, &str)],
    body: &Value,
    accept: &str,
) -> Result<HttpResponse, DocentError> {
    let payload = serde_json::to_string(body).map_err(|error| DocentError::ProviderTransport {
        message: format!("failed to encode request JSON: {error}"),
    })?;

    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(config.api.timeout_secs)))
            .http_status_as_error(false)
            .user_agent(format!("docent/{}", env!("CARGO_PKG_VERSION")))
            .build(),
    );

    let mut request = agent
        .post(url)
        .header("content-type", "application/json")
        .header("accept", accept);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    if config.api.base_url.contains("opencode.ai") {
        if let Some(session) = opencode_session_id() {
            request = request.header("x-opencode-session", session);
        }
    }

    let mut response =
        request
            .send(payload.as_bytes())
            .map_err(|error| DocentError::ProviderTransport {
                message: error.to_string(),
            })?;
    let status = response.status().as_u16();

    if !(200..300).contains(&status) {
        let text = response.body_mut().read_to_string().map_err(|error| {
            DocentError::ProviderTransport {
                message: format!("failed to read error response: {error}"),
            }
        })?;
        return Err(DocentError::ProviderHttp {
            status,
            message: provider_error_message(&text),
        });
    }

    Ok(response)
}

pub(crate) fn send_json(
    config: &Config,
    url: &str,
    headers: &[(&str, &str)],
    body: &Value,
) -> Result<Value, DocentError> {
    let mut response = post_json(config, url, headers, body, "application/json")?;
    let text =
        response
            .body_mut()
            .read_to_string()
            .map_err(|error| DocentError::ProviderTransport {
                message: format!("failed to read response body: {error}"),
            })?;

    serde_json::from_str(&text).map_err(|error| DocentError::ProviderTransport {
        message: format!(
            "failed to parse response JSON: {error}; body: {}",
            truncate(&text, 300)
        ),
    })
}

pub(crate) fn send_stream(
    config: &Config,
    url: &str,
    headers: &[(&str, &str)],
    body: &Value,
) -> Result<HttpResponse, DocentError> {
    post_json(config, url, headers, body, "text/event-stream")
}

pub(crate) fn provider_error_message(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
            return message.to_string();
        }
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            return message.to_string();
        }
    }

    let body = body.trim();
    if body.is_empty() {
        "empty response body".to_string()
    } else {
        truncate(body, 500)
    }
}

pub(crate) fn truncate(text: &str, max_chars: usize) -> String {
    let mut output: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        output.push_str("...");
    }
    output
}

fn opencode_session_id() -> Option<String> {
    let dirs = ProjectDirs::from("", "", "docent")?;
    let path = dirs.state_dir()?.join("opencode-session");
    load_or_create_session_id(&path)
}

fn load_or_create_session_id(path: &Path) -> Option<String> {
    if let Ok(id) = fs::read_to_string(path) {
        let id = id.trim();
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok()?;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let id = format!("{nanos:032x}{:08x}", std::process::id());
    fs::write(path, &id).ok()?;
    Some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_openai_error_messages() {
        let body = r#"{"error":{"message":"bad key","type":"auth"}}"#;
        assert_eq!(provider_error_message(body), "bad key");
    }

    #[test]
    fn extracts_anthropic_error_messages() {
        let body = r#"{"type":"error","error":{"type":"auth","message":"bad key"}}"#;
        assert_eq!(provider_error_message(body), "bad key");
    }

    #[test]
    fn falls_back_to_a_truncated_body() {
        let message = provider_error_message("plain text error");
        assert_eq!(message, "plain text error");
    }

    #[test]
    fn truncates_long_messages() {
        let message = truncate("abcdefghij", 4);
        assert_eq!(message, "abcd...");
    }

    #[test]
    fn usage_is_none_when_empty() {
        assert_eq!(Usage::new(None, None), None);
    }

    #[test]
    fn session_id_is_stable() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state/opencode-session");
        let first = load_or_create_session_id(&path).unwrap();
        let second = load_or_create_session_id(&path).unwrap();
        assert_eq!(first, second);
        assert!(!first.is_empty());
        assert!(path.exists());
    }

    #[test]
    fn environment_key_wins_over_the_literal_key() {
        let api = ApiConfig {
            api_key: Some("literal".to_string()),
            ..ApiConfig::default()
        };

        let key = resolve_api_key(&api, Some("from-env".to_string())).unwrap();
        assert_eq!(key, "from-env");
    }

    #[test]
    fn literal_key_is_used_when_the_environment_is_unset() {
        let api = ApiConfig {
            api_key: Some("literal".to_string()),
            ..ApiConfig::default()
        };

        let key = resolve_api_key(&api, None).unwrap();
        assert_eq!(key, "literal");
    }

    #[test]
    fn blank_environment_value_falls_back_to_the_literal_key() {
        let api = ApiConfig {
            api_key: Some("literal".to_string()),
            ..ApiConfig::default()
        };

        let key = resolve_api_key(&api, Some("  ".to_string())).unwrap();
        assert_eq!(key, "literal");
    }

    #[test]
    fn missing_key_reports_both_sources() {
        let api = ApiConfig::default();

        let error = resolve_api_key(&api, None).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("api.api_key"));
        assert!(message.contains("OPENCODE_API_KEY"));
    }

    #[test]
    fn missing_key_without_an_env_name_reports_only_the_literal_source() {
        let mut api = ApiConfig::default();
        api.api_key_env.clear();

        let error = resolve_api_key(&api, None).unwrap_err();
        assert_eq!(
            error.to_string(),
            "missing API key: set api.api_key in config.toml"
        );
    }

    #[test]
    fn stream_events_have_the_expected_json_shape() {
        assert_eq!(
            serde_json::to_string(&StreamEvent::Delta {
                text: "hi".to_string()
            })
            .unwrap(),
            r#"{"type":"delta","text":"hi"}"#
        );
        assert_eq!(
            serde_json::to_string(&StreamEvent::Html {
                html: "<p>hi</p>".to_string()
            })
            .unwrap(),
            r#"{"type":"html","html":"<p>hi</p>"}"#
        );
        assert_eq!(
            serde_json::to_string(&StreamEvent::Done {
                model: Some("test-model".to_string()),
                usage: Some(Usage {
                    input_tokens: Some(3),
                    output_tokens: Some(4),
                }),
                cached: false,
            })
            .unwrap(),
            r#"{"type":"done","model":"test-model","usage":{"input_tokens":3,"output_tokens":4}}"#
        );
        assert_eq!(
            serde_json::to_string(&StreamEvent::Error {
                message: "boom".to_string()
            })
            .unwrap(),
            r#"{"type":"error","message":"boom"}"#
        );
    }
}
