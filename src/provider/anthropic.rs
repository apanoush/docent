//! Anthropic Messages API client.

use serde_json::{Value, json};

use super::stream::stream_sse;
use super::{
    ChatInput, Completion, Usage, hit_output_limit, provider_error_message, send_json, send_stream,
    truncate,
};
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
        &body(input, false),
    )?;
    parse_response(&response)
}

pub(crate) fn complete_streaming(
    config: &Config,
    input: &ChatInput<'_>,
    api_key: &str,
    on_delta: &mut dyn FnMut(&str) -> Result<(), DocentError>,
) -> Result<Completion, DocentError> {
    let url = endpoint(config);
    let mut response = send_stream(
        config,
        &url,
        &[
            ("x-api-key", api_key),
            ("anthropic-version", ANTHROPIC_VERSION),
        ],
        &body(input, true),
    )?;

    let mut text = String::new();
    let mut model = None;
    let mut input_tokens = None;
    let mut output_tokens = None;
    let mut stop_reason = None;

    {
        let mut reader = response.body_mut().as_reader();
        stream_sse(&mut reader, &mut |data| {
            let value: Value =
                serde_json::from_str(data).map_err(|error| DocentError::ProviderTransport {
                    message: format!(
                        "failed to parse stream event: {error}; data: {}",
                        truncate(data, 300)
                    ),
                })?;

            match value.get("type").and_then(Value::as_str) {
                Some("message_start") => {
                    if let Some(message) = value.get("message") {
                        if let Some(value_model) = message.get("model").and_then(Value::as_str) {
                            model = Some(value_model.to_string());
                        }
                        if let Some(usage) = message.get("usage") {
                            input_tokens = usage.get("input_tokens").and_then(Value::as_u64);
                        }
                    }
                }
                Some("content_block_delta") => {
                    if let Some(delta) = value.pointer("/delta/text").and_then(Value::as_str)
                        && !delta.is_empty()
                    {
                        text.push_str(delta);
                        on_delta(delta)?;
                    }
                }
                Some("message_delta") => {
                    if let Some(usage) = value.get("usage") {
                        output_tokens = usage.get("output_tokens").and_then(Value::as_u64);
                    }
                    if let Some(reason) =
                        value.pointer("/delta/stop_reason").and_then(Value::as_str)
                    {
                        stop_reason = Some(reason.to_string());
                    }
                }
                Some("error") => {
                    return Err(DocentError::ProviderTransport {
                        message: provider_error_message(&value.to_string()),
                    });
                }
                _ => {}
            }

            Ok(())
        })?;
    }

    if text.trim().is_empty() {
        return Err(DocentError::EmptyResponse);
    }

    Ok(Completion {
        text,
        model,
        usage: Usage::new(input_tokens, output_tokens),
        cached: false,
        truncated: stop_reason.as_deref().is_some_and(hit_output_limit),
    })
}

fn endpoint(config: &Config) -> String {
    let base = config.provider.base_url.trim_end_matches('/');
    if base.ends_with("/v1") {
        format!("{base}/messages")
    } else {
        format!("{base}/v1/messages")
    }
}

fn body(input: &ChatInput<'_>, stream: bool) -> Value {
    json!({
        "model": input.model,
        "system": input.system,
        "messages": [{"role": "user", "content": input.user}],
        "temperature": input.temperature,
        "max_tokens": input.max_tokens,
        "stream": stream
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
        cached: false,
        truncated: value
            .get("stop_reason")
            .and_then(Value::as_str)
            .is_some_and(hit_output_limit),
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
        let value = body(&input(), false);
        assert_eq!(value["model"], "test-model");
        assert_eq!(value["system"], "system");
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"], "user");
        assert!((value["temperature"].as_f64().unwrap() - 0.2).abs() < 1e-6);
        assert_eq!(value["max_tokens"], 64);
        assert_eq!(value["stream"], false);
    }

    #[test]
    fn streaming_body_enables_streaming() {
        let value = body(&input(), true);
        assert_eq!(value["stream"], true);
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
    fn marks_an_answer_that_hit_the_output_limit() {
        let value = json!({
            "stop_reason": "max_tokens",
            "content": [{"type": "text", "text": "cut"}]
        });
        let completion = parse_response(&value).unwrap();
        assert!(completion.truncated);
    }

    #[test]
    fn does_not_mark_a_finished_answer() {
        let value = json!({
            "stop_reason": "end_turn",
            "content": [{"type": "text", "text": "done"}]
        });
        let completion = parse_response(&value).unwrap();
        assert!(!completion.truncated);
    }

    #[test]
    fn endpoint_handles_both_base_url_styles() {
        let mut config = Config::default();
        config.provider.base_url = "https://api.anthropic.com".to_string();
        assert_eq!(endpoint(&config), "https://api.anthropic.com/v1/messages");

        config.provider.base_url = "https://api.anthropic.com/v1".to_string();
        assert_eq!(endpoint(&config), "https://api.anthropic.com/v1/messages");
    }
}
