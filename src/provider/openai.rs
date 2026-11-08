//! OpenAI Chat Completions client.

use serde_json::{Value, json};

use super::stream::stream_sse;
use super::{
    ChatInput, Completion, Usage, provider_error_message, send_json, send_stream, truncate,
};
use crate::config::Config;
use crate::error::DocentError;

pub(crate) fn complete(
    config: &Config,
    input: &ChatInput<'_>,
    api_key: &str,
) -> Result<Completion, DocentError> {
    let authorization = format!("Bearer {api_key}");
    let url = endpoint(config);
    let response = send_json(
        config,
        &url,
        &[("authorization", authorization.as_str())],
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
    let authorization = format!("Bearer {api_key}");
    let url = endpoint(config);
    let mut response = send_stream(
        config,
        &url,
        &[("authorization", authorization.as_str())],
        &body(input, true),
    )?;

    let mut text = String::new();
    let mut model = None;
    let mut usage = None;
    let mut finished = false;

    {
        let mut reader = response.body_mut().as_reader();
        stream_sse(&mut reader, &mut |data| {
            if finished {
                return Ok(());
            }
            if data == "[DONE]" {
                finished = true;
                return Ok(());
            }

            let value: Value =
                serde_json::from_str(data).map_err(|error| DocentError::ProviderTransport {
                    message: format!(
                        "failed to parse stream event: {error}; data: {}",
                        truncate(data, 300)
                    ),
                })?;
            if value.get("error").is_some() {
                return Err(DocentError::ProviderTransport {
                    message: provider_error_message(&value.to_string()),
                });
            }
            if let Some(value_model) = value.get("model").and_then(Value::as_str) {
                model = Some(value_model.to_string());
            }
            if let Some(value_usage) = parse_usage(&value) {
                usage = Some(value_usage);
            }
            if let Some(content) = value.pointer("/choices/0/delta/content") {
                let delta = content_text(content);
                if !delta.is_empty() {
                    text.push_str(&delta);
                    on_delta(&delta)?;
                }
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
        usage,
        cached: false,
    })
}

fn endpoint(config: &Config) -> String {
    format!(
        "{}/chat/completions",
        config.api.base_url.trim_end_matches('/')
    )
}

fn body(input: &ChatInput<'_>, stream: bool) -> Value {
    json!({
        "model": input.model,
        "messages": [
            {"role": "system", "content": input.system},
            {"role": "user", "content": input.user}
        ],
        "temperature": input.temperature,
        "max_tokens": input.max_tokens,
        "stream": stream
    })
}

fn parse_response(value: &Value) -> Result<Completion, DocentError> {
    let content = value.pointer("/choices/0/message/content").ok_or_else(|| {
        DocentError::ProviderTransport {
            message: "provider response is missing choices[0].message.content".to_string(),
        }
    })?;

    let text = match content {
        Value::String(_) | Value::Array(_) => content_text(content),
        _ => {
            return Err(DocentError::ProviderTransport {
                message: "provider returned an unexpected content shape".to_string(),
            });
        }
    };

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
    })
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<String>(),
        _ => String::new(),
    }
}

fn parse_usage(value: &Value) -> Option<Usage> {
    let usage = value.get("usage")?;
    Usage::new(
        usage.get("prompt_tokens").and_then(Value::as_u64),
        usage.get("completion_tokens").and_then(Value::as_u64),
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
        assert_eq!(value["messages"][0]["role"], "system");
        assert_eq!(value["messages"][0]["content"], "system");
        assert_eq!(value["messages"][1]["role"], "user");
        assert_eq!(value["messages"][1]["content"], "user");
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
    fn parses_a_string_completion() {
        let value = json!({
            "model": "test-model",
            "choices": [{"message": {"role": "assistant", "content": "hello"}}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 4}
        });
        let completion = parse_response(&value).unwrap();
        assert_eq!(completion.text, "hello");
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
    fn parses_an_array_completion() {
        let value = json!({
            "choices": [{"message": {"content": [{"type": "text", "text": "hello "}, {"type": "text", "text": "world"}]}}]
        });
        let completion = parse_response(&value).unwrap();
        assert_eq!(completion.text, "hello world");
    }

    #[test]
    fn rejects_missing_content() {
        let error = parse_response(&json!({"choices": []})).unwrap_err();
        assert!(error.to_string().contains("choices"));
    }

    #[test]
    fn rejects_empty_content() {
        let value = json!({"choices": [{"message": {"content": "   "}}]});
        let error = parse_response(&value).unwrap_err();
        assert!(matches!(error, DocentError::EmptyResponse));
    }
}
