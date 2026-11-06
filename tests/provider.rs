mod common;

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use common::{MockResponse, MockServer};
use serde_json::Value;

fn write_config(dir: &Path, kind: &str, base_url: &str, context_mode: &str, max_chars: usize) {
    write_config_with_key(
        dir,
        kind,
        base_url,
        context_mode,
        max_chars,
        r#"api_key_env = "DOCENT_TEST_KEY""#,
    );
}

fn write_config_with_key(
    dir: &Path,
    kind: &str,
    base_url: &str,
    context_mode: &str,
    max_chars: usize,
    key_line: &str,
) {
    fs::create_dir_all(dir.join("profiles")).unwrap();
    let config = format!(
        r#"[api]
kind = "{kind}"
base_url = "{base_url}"
model = "test-model"
{key_line}
timeout_secs = 5
max_tokens = 64
temperature = 0.2

[context]
mode = "{context_mode}"
max_chars = {max_chars}

[response]
language = "auto"
target_language = "English"
"#
    );
    fs::write(dir.join("config.toml"), config).unwrap();
}

fn write_profile(dir: &Path, name: &str, contents: &str) {
    fs::write(dir.join("profiles").join(format!("{name}.toml")), contents).unwrap();
}

fn setup(dir: &Path, kind: &str, base_url: &str) {
    setup_with_key(dir, kind, base_url, r#"api_key_env = "DOCENT_TEST_KEY""#);
}

fn setup_with_key(dir: &Path, kind: &str, base_url: &str, key_line: &str) {
    write_config_with_key(dir, kind, base_url, "full", 32_000, key_line);
    write_profile(
        dir,
        "explain",
        r#"description = "Explain"
requires = ["selection"]

user_prompt = """
<document>
{{context}}
</document>
<selection>
{{selection}}
</selection>
Respond in {{response_language}}.
"""
"#,
    );
    write_profile(
        dir,
        "ask",
        r#"description = "Ask"
requires = ["selection", "question"]

user_prompt = """
Q: {{question}}
D: {{context}}
S: {{selection}}
"""
"#,
    );
}

fn run(config_dir: &Path, args: &[&str], stdin: &str, key: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_docent"));
    command
        .args(["--config-dir", config_dir.to_str().unwrap()])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(key) = key {
        command.env("DOCENT_TEST_KEY", key);
    } else {
        command.env_remove("DOCENT_TEST_KEY");
    }

    let mut child = command.spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(stdin.as_bytes()).unwrap();
    drop(input);
    child.wait_with_output().unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn openai_request_and_json_output() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"model":"test-model","choices":[{"message":{"role":"assistant","content":"Hello **world**"}}],"usage":{"prompt_tokens":3,"completion_tokens":4}}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let input = r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10,"file":"notes.md"}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain", "--json"],
        input,
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["text"], "Hello **world**");
    assert_eq!(value["model"], "test-model");
    assert_eq!(value["usage"]["input_tokens"], 3);
    assert_eq!(value["usage"]["output_tokens"], 4);

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/chat/completions");
    assert_eq!(requests[0].header("authorization"), Some("Bearer secret"));
    assert!(
        requests[0]
            .header("user-agent")
            .unwrap()
            .starts_with("docent/")
    );

    let body = requests[0].json();
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["stream"], false);
    assert_eq!(body["max_tokens"], 64);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("beta"));
    assert!(user.contains("alpha"));
    assert!(user.contains("SELECTION START"));
}

#[test]
fn anthropic_request_and_plain_output() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"model":"test-model","content":[{"type":"text","text":"Bonjour"},{"type":"text","text":" le monde"}],"usage":{"input_tokens":5,"output_tokens":6}}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "anthropic", &server.base_url());

    let input =
        r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain"],
        input,
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Bonjour le monde\n"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/messages");
    assert_eq!(requests[0].header("x-api-key"), Some("secret"));
    assert_eq!(requests[0].header("anthropic-version"), Some("2023-06-01"));

    let body = requests[0].json();
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["stream"], false);
    assert_eq!(body["max_tokens"], 64);
    assert!(body["system"].as_str().unwrap().contains("Docent"));
    assert_eq!(body["messages"][0]["role"], "user");
    assert!(
        body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("beta")
    );
}

#[test]
fn literal_openai_key_is_sent_without_an_env_var() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"ok"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup_with_key(
        &dir,
        "openai",
        &server.base_url(),
        r#"api_key = "literal-secret""#,
    );

    let input =
        r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10}"#;
    let output = run(&dir, &["run", "--profile", "explain"], input, None);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let requests = server.requests();
    assert_eq!(
        requests[0].header("authorization"),
        Some("Bearer literal-secret")
    );
}

#[test]
fn literal_anthropic_key_is_sent_without_an_env_var() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"content":[{"type":"text","text":"ok"}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup_with_key(
        &dir,
        "anthropic",
        &server.base_url(),
        r#"api_key = "literal-secret""#,
    );

    let input =
        r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10}"#;
    let output = run(&dir, &["run", "--profile", "explain"], input, None);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let requests = server.requests();
    assert_eq!(requests[0].header("x-api-key"), Some("literal-secret"));
}

#[test]
fn print_prompt_does_not_call_the_provider() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", "http://127.0.0.1:1");

    let input =
        r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain", "--print-prompt"],
        input,
        None,
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["profile"], "explain");
    assert_eq!(value["model"], "test-model");
    assert_eq!(value["context_truncated"], false);
    assert!(value["system"].as_str().unwrap().contains("Docent"));
    let user = value["user"].as_str().unwrap();
    assert!(user.contains("beta"));
    assert!(user.contains("alpha"));
    assert!(user.contains("SELECTION START"));
    assert!(user.contains("the same language as the selected passage"));
}

#[test]
fn auto_context_truncation_is_reported_by_print_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    write_config(&dir, "openai", "http://127.0.0.1:1", "auto", 20);
    write_profile(
        &dir,
        "explain",
        r#"requires = ["selection"]
user_prompt = "{{selection}}\n{{context}}"
"#,
    );

    let document = "first line\nsecond line\nthird line\nfourth line\nfifth line";
    let start = document.find("third").unwrap();
    let input = format!(
        r#"{{"selection":"third","document":{},"byte_start":{start},"byte_end":{}}}"#,
        serde_json::to_string(document).unwrap(),
        start + "third".len()
    );

    let output = run(
        &dir,
        &["run", "--profile", "explain", "--print-prompt"],
        &input,
        None,
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["context_truncated"], true);
    assert!(value["user"].as_str().unwrap().contains("TRUNCATED"));
}

#[test]
fn ask_question_is_passed_through() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"because"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let input =
        r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10}"#;
    let output = run(
        &dir,
        &["run", "--profile", "ask", "--question", "Why?"],
        input,
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let requests = server.requests();
    let body = requests[0].json();
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("Why?"));
    assert!(user.contains("beta"));
}

#[test]
fn missing_question_exits_with_input_error() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", "http://127.0.0.1:1");

    let output = run(
        &dir,
        &["run", "--profile", "ask"],
        r#"{"selection":"beta"}"#,
        None,
    );
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("question"));
}

#[test]
fn missing_api_key_is_reported() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", "http://127.0.0.1:1");

    let input =
        r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10}"#;
    let output = run(&dir, &["run", "--profile", "explain"], input, None);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("DOCENT_TEST_KEY"));
}

#[test]
fn http_error_is_reported() {
    let server = MockServer::start(MockResponse::json(
        401,
        r#"{"error":{"message":"bad key"}}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let input =
        r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain"],
        input,
        Some("secret"),
    );
    assert_eq!(output.status.code(), Some(3));
    let message = stderr(&output);
    assert!(message.contains("HTTP 401"));
    assert!(message.contains("bad key"));
}

#[test]
fn malformed_provider_json_is_reported() {
    let server = MockServer::start(MockResponse::json(200, "not json"));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let input =
        r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain"],
        input,
        Some("secret"),
    );
    assert_eq!(output.status.code(), Some(3));
    assert!(stderr(&output).contains("failed to parse response JSON"));
}

#[test]
fn invalid_request_json_is_reported() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", "http://127.0.0.1:1");

    let output = run(&dir, &["run", "--profile", "explain"], "not json", None);
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("failed to parse request JSON"));
}

#[test]
fn empty_stdin_is_reported() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", "http://127.0.0.1:1");

    let output = run(&dir, &["run", "--profile", "explain"], "", None);
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("expected a JSON request"));
}

#[test]
fn unknown_request_fields_are_reported() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", "http://127.0.0.1:1");

    let output = run(
        &dir,
        &["run", "--profile", "explain"],
        r#"{"selection":"beta","extra":1}"#,
        None,
    );
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("extra"));
}

#[test]
fn openai_streaming_events() {
    let body = concat!(
        "data: {\"model\":\"test-model\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{}}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n",
        "data: [DONE]\n\n"
    );
    let server = MockServer::start(MockResponse::sse(200, body));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let input =
        r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain", "--stream"],
        input,
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0]["type"], "delta");
    assert_eq!(events[0]["text"], "Hello");
    assert_eq!(events[1]["text"], " world");
    assert_eq!(events[2]["type"], "done");
    assert_eq!(events[2]["model"], "test-model");
    assert_eq!(events[2]["usage"]["input_tokens"], 3);
    assert_eq!(events[2]["usage"]["output_tokens"], 2);

    let requests = server.requests();
    assert_eq!(requests[0].header("accept"), Some("text/event-stream"));
    let request = requests[0].json();
    assert_eq!(request["stream"], true);
}

#[test]
fn anthropic_streaming_events() {
    let body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"model\":\"test-model\",\"usage\":{\"input_tokens\":3}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Bonjour\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\" le monde\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":4}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );
    let server = MockServer::start(MockResponse::sse(200, body));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "anthropic", &server.base_url());

    let input =
        r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain", "--stream"],
        input,
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0]["type"], "delta");
    assert_eq!(events[0]["text"], "Bonjour");
    assert_eq!(events[1]["text"], " le monde");
    assert_eq!(events[2]["type"], "done");
    assert_eq!(events[2]["model"], "test-model");
    assert_eq!(events[2]["usage"]["input_tokens"], 3);
    assert_eq!(events[2]["usage"]["output_tokens"], 4);
}

#[test]
fn streaming_http_error_emits_an_error_event() {
    let server = MockServer::start(MockResponse::json(
        401,
        r#"{"error":{"message":"bad key"}}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let input =
        r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain", "--stream"],
        input,
        Some("secret"),
    );
    assert_eq!(output.status.code(), Some(3));

    let stderr_text = stderr(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let event: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(event["type"], "error");
    assert!(event["message"].as_str().unwrap().contains("HTTP 401"));
    assert!(event["message"].as_str().unwrap().contains("bad key"));
    assert!(stderr_text.contains("HTTP 401"));
}

fn write_notes(directory: &Path, contents: &str) -> std::path::PathBuf {
    let path = directory.join("notes.md");
    fs::write(&path, contents).unwrap();
    path
}

#[test]
fn explain_file_lines_streams_plain_text() {
    let server = MockServer::start(MockResponse::sse(
        200,
        concat!(
            "data: {\"model\":\"test-model\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
            "data: [DONE]\n\n"
        ),
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    let notes = write_notes(temp.path(), "alpha\nbeta gamma\ndelta\n");

    let output = run(
        &dir,
        &["explain", notes.to_str().unwrap(), "--lines", "2:3"],
        "",
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "Hello world\n");

    let requests = server.requests();
    let body = requests[0].json();
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("beta gamma"));
    assert!(user.contains("delta"));
    assert_eq!(body["stream"], true);
}

#[test]
fn explain_no_stream_buffers_the_answer() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"Buffered answer"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    let notes = write_notes(temp.path(), "alpha\nbeta gamma\ndelta\n");

    let output = run(
        &dir,
        &[
            "explain",
            notes.to_str().unwrap(),
            "--line",
            "2",
            "--no-stream",
        ],
        "",
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Buffered answer\n"
    );

    let body = server.requests()[0].json();
    assert_eq!(body["stream"], false);
    assert!(
        body["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("beta gamma")
    );
}

#[test]
fn ask_question_is_added_to_the_prompt() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"because"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    let notes = write_notes(temp.path(), "alpha\nbeta gamma\n");

    let output = run(
        &dir,
        &[
            "ask",
            notes.to_str().unwrap(),
            "--line",
            "1",
            "--question",
            "Why?",
            "--no-stream",
        ],
        "",
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let body = server.requests()[0].json();
    assert!(
        body["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("Why?")
    );
}

#[test]
fn translate_to_sets_the_target_language() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"Bonjour"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    write_profile(
        &dir,
        "translate",
        r#"description = "Translate"
requires = ["selection"]
user_prompt = "Translate into {{target_language}}: {{selection}}"
"#,
    );
    let notes = write_notes(temp.path(), "alpha\nbeta gamma\n");

    let output = run(
        &dir,
        &[
            "translate",
            notes.to_str().unwrap(),
            "--line",
            "1",
            "--to",
            "French",
            "--no-stream",
        ],
        "",
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let body = server.requests()[0].json();
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("French"));
    assert!(user.contains("alpha"));
}

#[test]
fn do_runs_a_custom_profile() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"custom answer"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    write_profile(
        &dir,
        "custom",
        r#"requires = ["selection"]
user_prompt = "Custom {{selection}}"
"#,
    );
    let notes = write_notes(temp.path(), "alpha\nbeta gamma\n");

    let output = run(
        &dir,
        &[
            "do",
            "custom",
            notes.to_str().unwrap(),
            "--line",
            "2",
            "--no-stream",
        ],
        "",
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let body = server.requests()[0].json();
    assert!(
        body["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("Custom beta gamma")
    );
}

#[test]
fn explain_reads_the_document_from_stdin() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"from stdin"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let output = run(
        &dir,
        &["explain", "-", "--line", "2", "--no-stream"],
        "alpha\nbeta\n",
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let body = server.requests()[0].json();
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("beta"));
    assert!(user.contains("alpha"));
}

#[test]
fn explain_uses_text_with_file_context() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"text answer"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    let notes = write_notes(temp.path(), "alpha beta gamma\n");

    let output = run(
        &dir,
        &[
            "explain",
            notes.to_str().unwrap(),
            "--text",
            "beta",
            "--no-stream",
        ],
        "",
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let body = server.requests()[0].json();
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("beta"));
    assert!(user.contains("alpha"));
}

#[test]
fn explain_selects_a_phrase() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"phrase answer"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    let notes = write_notes(temp.path(), "alpha beta gamma\n");

    let output = run(
        &dir,
        &[
            "explain",
            notes.to_str().unwrap(),
            "--select",
            "beta",
            "--no-stream",
        ],
        "",
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let body = server.requests()[0].json();
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("SELECTION START"));
    assert!(user.contains("beta"));
}

#[test]
fn missing_phrase_is_an_input_error() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", "http://127.0.0.1:1");
    let notes = write_notes(temp.path(), "alpha beta gamma\n");

    let output = run(
        &dir,
        &["explain", notes.to_str().unwrap(), "--select", "missing"],
        "",
        None,
    );
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("phrase not found"));
}

#[test]
fn out_of_bounds_lines_are_an_input_error() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", "http://127.0.0.1:1");
    let notes = write_notes(temp.path(), "alpha\nbeta\n");

    let output = run(
        &dir,
        &["explain", notes.to_str().unwrap(), "--lines", "9:10"],
        "",
        None,
    );
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("out of bounds"));
}

#[test]
fn missing_selection_is_an_input_error() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", "http://127.0.0.1:1");
    let notes = write_notes(temp.path(), "alpha\nbeta\n");

    let output = run(
        &dir,
        &["explain", notes.to_str().unwrap(), "--no-stream"],
        "",
        None,
    );
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("no selection provided"));
}

#[test]
fn missing_file_is_an_input_error() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", "http://127.0.0.1:1");
    let missing = temp.path().join("missing.md");

    let output = run(
        &dir,
        &["explain", missing.to_str().unwrap(), "--line", "1"],
        "",
        None,
    );
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("failed to read"));
}

#[test]
fn missing_question_is_an_input_error_without_a_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", "http://127.0.0.1:1");
    let notes = write_notes(temp.path(), "alpha\nbeta\n");

    let output = run(
        &dir,
        &["ask", notes.to_str().unwrap(), "--line", "1", "--no-stream"],
        "",
        None,
    );
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("requires a question"));
}
