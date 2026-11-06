//! OpenAI Chat Completions client.

use serde_json::{Value, json};

use super::{ChatInput, Completion, Usage, send_json};
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
        &body(input),
    )?;
    parse_response(&response)
}

fn endpoint(config: &Config) -> String {
    format!(
        "{}/chat/completions",
        config.api.base_url.trim_end_matches('/')
    )
}

fn body(input: &ChatInput<'_>) -> Value {
    json!({
        "model": input.model,
        "messages": [
            {"role": "system", "content": input.system},
            {"role": "user", "content": input.user}
        ],
        "temperature": input.temperature,
        "max_tokens": input.max_tokens,
        "stream": false
    })
}

fn parse_response(value: &Value) -> Result<Completion, DocentError> {
    let content = value.pointer("/choices/0/message/content").ok_or_else(|| {
        DocentError::ProviderTransport {
            message: "provider response is missing choices[0].message.content".to_string(),
        }
    })?;

    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<String>(),
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
    })
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
        let value = body(&input());
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
