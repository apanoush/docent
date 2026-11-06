//! Anthropic Messages API client.

use serde_json::{Value, json};

use super::{ChatInput, Completion, Usage, send_json};
use crate::config::Config;
use crate::error::DocentError;

const ANTHROPIC_VERSION: &str = "2023-06-01";

pub(crate) fn complete(
    config: &Config,
    input: &ChatInput<'_>,
    api_key: &str,
) -> Result<Completion, DocentError> {
    let url = endpoint(config);
    let response = send_json(
        config,
        &url,
        &[
            ("x-api-key", api_key),
            ("anthropic-version", ANTHROPIC_VERSION),
        ],
        &body(input),
    )?;
    parse_response(&response)
}

fn endpoint(config: &Config) -> String {
    let base = config.api.base_url.trim_end_matches('/');
    if base.ends_with("/v1") {
        format!("{base}/messages")
    } else {
        format!("{base}/v1/messages")
    }
}

fn body(input: &ChatInput<'_>) -> Value {
    json!({
        "model": input.model,
        "system": input.system,
        "messages": [{"role": "user", "content": input.user}],
        "temperature": input.temperature,
        "max_tokens": input.max_tokens,
        "stream": false
    })
}

fn parse_response(value: &Value) -> Result<Completion, DocentError> {
    let text = value
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .unwrap_or_default();

    if text.trim().is_empty() {
        return Err(DocentError::EmptyResponse);
    }

    Ok(Completion {
        text,
        model: value
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string),
        usage: parse_usage(value),
    })
}

fn parse_usage(value: &Value) -> Option<Usage> {
    let usage = value.get("usage")?;
    Usage::new(
        usage.get("input_tokens").and_then(Value::as_u64),
        usage.get("output_tokens").and_then(Value::as_u64),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> ChatInput<'static> {
        ChatInput {
            system: "system",
            user: "user",
            model: "test-model",
            temperature: 0.2,
            max_tokens: 64,
        }
    }

    #[test]
    fn request_body_has_the_expected_shape() {
        let value = body(&input());
        assert_eq!(value["model"], "test-model");
        assert_eq!(value["system"], "system");
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"], "user");
        assert!((value["temperature"].as_f64().unwrap() - 0.2).abs() < 1e-6);
        assert_eq!(value["max_tokens"], 64);
        assert_eq!(value["stream"], false);
    }

    #[test]
    fn parses_multiple_text_blocks() {
        let value = json!({
            "model": "test-model",
            "content": [
                {"type": "thinking", "thinking": "internal"},
                {"type": "text", "text": "hello "},
                {"type": "text", "text": "world"}
            ],
            "usage": {"input_tokens": 3, "output_tokens": 4}
        });
        let completion = parse_response(&value).unwrap();
        assert_eq!(completion.text, "hello world");
        assert_eq!(completion.model.as_deref(), Some("test-model"));
        assert_eq!(
            completion.usage,
            Some(Usage {
                input_tokens: Some(3),
                output_tokens: Some(4)
            })
        );
    }

    #[test]
    fn rejects_empty_content() {
        let error = parse_response(&json!({"content": []})).unwrap_err();
        assert!(matches!(error, DocentError::EmptyResponse));
    }

    #[test]
    fn endpoint_handles_both_base_url_styles() {
        let mut config = Config::default();
        config.api.base_url = "https://api.anthropic.com".to_string();
        assert_eq!(endpoint(&config), "https://api.anthropic.com/v1/messages");

        config.api.base_url = "https://api.anthropic.com/v1".to_string();
        assert_eq!(endpoint(&config), "https://api.anthropic.com/v1/messages");
    }
}
