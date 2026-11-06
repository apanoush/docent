//! Provider clients for OpenAI-compatible and Anthropic APIs.

pub mod anthropic;
pub mod openai;

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::config::{ApiKind, Config};
use crate::error::DocentError;
use crate::prompt::{EffectiveSettings, Prompt};

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
    let api_key =
        std::env::var(&config.api.api_key_env).map_err(|_| DocentError::MissingApiKey {
            env: config.api.api_key_env.clone(),
        })?;

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

pub(crate) fn send_json(
    config: &Config,
    url: &str,
    headers: &[(&str, &str)],
    body: &Value,
) -> Result<Value, DocentError> {
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
        .header("accept", "application/json");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }

    let mut response =
        request
            .send(payload.as_bytes())
            .map_err(|error| DocentError::ProviderTransport {
                message: error.to_string(),
            })?;
    let status = response.status().as_u16();
    let text =
        response
            .body_mut()
            .read_to_string()
            .map_err(|error| DocentError::ProviderTransport {
                message: format!("failed to read response body: {error}"),
            })?;

    if !(200..300).contains(&status) {
        return Err(DocentError::ProviderHttp {
            status,
            message: provider_error_message(&text),
        });
    }

    serde_json::from_str(&text).map_err(|error| DocentError::ProviderTransport {
        message: format!(
            "failed to parse response JSON: {error}; body: {}",
            truncate(&text, 300)
        ),
    })
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
}
