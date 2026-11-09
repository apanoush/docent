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
    let database = dir
        .join("docent.db")
        .display()
        .to_string()
        .replace('\\', "\\\\");
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

[history]
database = "{database}"

[cache]
enabled = false
"#
    );
    fs::write(dir.join("config.toml"), config).unwrap();
}

fn enable_cache(dir: &Path) {
    let path = dir.join("config.toml");
    let contents = fs::read_to_string(&path).unwrap();
    let enabled = contents.replace("[cache]\nenabled = false", "[cache]\nenabled = true");
    assert!(enabled.contains("[cache]\nenabled = true"));
    fs::write(path, enabled).unwrap();
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
    write_profile(
        dir,
        "summarize",
        r#"description = "Summarize"
requires = ["context"]

user_prompt = """
<document>
{{context}}
</document>
If a passage is marked as the selection, summarize it. Otherwise summarize the document.
"""
"#,
    );
}

fn run(config_dir: &Path, args: &[&str], stdin: &str, key: Option<&str>) -> Output {
    run_with_env(config_dir, args, stdin, key, &[])
}

fn run_with_env(
    config_dir: &Path,
    args: &[&str],
    stdin: &str,
    key: Option<&str>,
    envs: &[(&str, &str)],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_docent"));
    command
        .args(["--config-dir", config_dir.to_str().unwrap()])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("DISPLAY", ":0");
    for (name, value) in envs {
        command.env(name, value);
    }

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
    assert!(user.contains("the same language as the source text"));
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
fn summarize_document_without_a_selection() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"A summary"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let notes = temp.path().join("notes.md");
    fs::write(&notes, "alpha beta gamma\n").unwrap();
    let output = run(
        &dir,
        &["summarize", notes.to_str().unwrap(), "--no-stream"],
        "",
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "A summary\n");

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let body = requests[0].json();
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("alpha beta gamma"));
    assert!(!user.contains("SELECTION START"));
}

#[test]
fn summarize_selected_lines_marks_the_selection() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"A summary"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let notes = temp.path().join("notes.md");
    fs::write(&notes, "alpha\nbeta\ngamma\n").unwrap();
    let output = run(
        &dir,
        &[
            "summarize",
            notes.to_str().unwrap(),
            "--lines",
            "2",
            "--no-stream",
        ],
        "",
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let requests = server.requests();
    let body = requests[0].json();
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("SELECTION START"));
    assert!(user.contains("beta"));
    assert!(user.contains("gamma"));
}

#[test]
fn truncated_document_without_a_selection_warns() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    write_config(&dir, "openai", "http://127.0.0.1:1", "auto", 20);
    write_profile(
        &dir,
        "summarize",
        r#"requires = ["context"]
user_prompt = "{{context}}"
"#,
    );

    let document = "first line\nsecond line\nthird line\nfourth line";
    let input = format!(
        r#"{{"document":{}}}"#,
        serde_json::to_string(document).unwrap()
    );
    let output = run(
        &dir,
        &["run", "--profile", "summarize", "--print-prompt"],
        &input,
        None,
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(stderr(&output).contains("truncated"));
    assert!(stderr(&output).contains("context.max_chars"));

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
fn html_streaming_event_renders_the_answer() {
    let body = concat!(
        "data: {\"model\":\"test-model\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
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
        &["run", "--profile", "explain", "--stream", "--html"],
        input,
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 4);
    assert_eq!(events[0]["type"], "delta");
    assert_eq!(events[1]["type"], "delta");
    assert_eq!(events[2]["type"], "html");
    assert_eq!(events[2]["html"], "<p>Hello world</p>\n");
    assert_eq!(events[3]["type"], "done");
}

#[test]
fn html_requires_a_stream() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", "http://127.0.0.1:1");

    let input =
        r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain", "--html"],
        input,
        Some("secret"),
    );
    assert_eq!(output.status.code(), Some(2));
    let error = stderr(&output);
    assert!(error.contains("--html"), "stderr: {error}");
    assert!(error.contains("--stream"), "stderr: {error}");
}

#[cfg(unix)]
fn write_fake_browser(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(
        path,
        r#"#!/usr/bin/env python3
import json
import os
import sys
import urllib.request

url = sys.argv[-1]
page = urllib.request.urlopen(url).read().decode("utf-8")
events = []
with urllib.request.urlopen(url.rstrip("/") + "/events") as response:
    for raw in response:
        line = raw.decode("utf-8").strip()
        if not line.startswith("data: "):
            continue
        event = json.loads(line[6:])
        events.append(event)
        if event["type"] in {"done", "error"}:
            break
with open(os.environ["FAKE_BROWSER_OUTPUT"], "w", encoding="utf-8") as handle:
    json.dump({"page": page, "events": events, "argv": sys.argv[1:]}, handle)
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

#[cfg(unix)]
fn append_viewer_config(dir: &Path, contents: &str) {
    let path = dir.join("config.toml");
    let mut config = fs::read_to_string(&path).unwrap();
    config.push_str(contents);
    fs::write(path, config).unwrap();
}

#[cfg(unix)]
fn check_browser_viewer(viewer: &str, browser_flag: &str, expected_target: &str) {
    let body = concat!(
        "data: {\"model\":\"test-model\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
        "data: [DONE]\n\n"
    );
    let server = MockServer::start(MockResponse::sse(200, body));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let notes = temp.path().join("notes.md");
    fs::write(&notes, "alpha beta gamma\n").unwrap();
    let css = temp.path().join("custom.css");
    fs::write(&css, ".markdown h2 { color: red; }").unwrap();
    let output_file = temp.path().join("browser.json");

    let browser = temp.path().join("fake-browser");
    write_fake_browser(&browser);

    let output = run_with_env(
        &dir,
        &[
            "explain",
            notes.to_str().unwrap(),
            "--lines",
            "1:1",
            "--viewer",
            viewer,
            browser_flag,
            browser.to_str().unwrap(),
            "--answer-css",
            css.to_str().unwrap(),
        ],
        "",
        Some("secret"),
        &[("FAKE_BROWSER_OUTPUT", output_file.to_str().unwrap())],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(output.stdout.is_empty());

    let captured: Value = serde_json::from_str(&fs::read_to_string(&output_file).unwrap()).unwrap();
    let page = captured["page"].as_str().unwrap();
    assert!(page.contains("docent: explain"));
    assert!(page.contains("notes.md (lines 1)"));
    assert!(page.contains(".markdown h2 { color: red; }"));

    let argv = captured["argv"].as_array().unwrap();
    assert_eq!(argv[0].as_str().unwrap(), expected_target);
    assert!(argv[1].as_str().unwrap().starts_with("http://127.0.0.1:"));

    let events = captured["events"].as_array().unwrap();
    let kinds: Vec<_> = events.iter().map(|event| event["type"].as_str()).collect();
    assert_eq!(
        kinds,
        vec![Some("delta"), Some("delta"), Some("html"), Some("done")]
    );
    assert_eq!(events[2]["html"], "<p>Hello world</p>\n");
    assert!(events[3]["label"].as_str().unwrap().contains("test-model"));

    let connection = rusqlite::Connection::open(history_database(&dir)).unwrap();
    let (status, mode): (String, String) = connection
        .query_row("SELECT status, output_mode FROM runs", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(status, "success");
    assert_eq!(mode, "browser");
}

#[cfg(unix)]
#[test]
fn browser_viewer_streams_into_qutebrowser() {
    check_browser_viewer("qutebrowser", "--qutebrowser", "--target=window");
}

#[cfg(unix)]
#[test]
fn browser_viewer_streams_into_firefox() {
    check_browser_viewer("firefox", "--firefox", "--new-window");
}

#[cfg(unix)]
fn check_config_viewer(viewer: &str, expected_target: &str) {
    let body = concat!(
        "data: {\"model\":\"test-model\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
        "data: [DONE]\n\n"
    );
    let server = MockServer::start(MockResponse::sse(200, body));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let notes = temp.path().join("notes.md");
    fs::write(&notes, "alpha beta gamma\n").unwrap();
    let output_file = temp.path().join("browser.json");
    let browser = temp.path().join("fake-browser");
    write_fake_browser(&browser);

    append_viewer_config(
        &dir,
        &format!(
            r#"
[viewer]
default = "{viewer}"
qutebrowser = "{}"
qutebrowser_target = "tab-bg"
firefox = "{}"
firefox_target = "tab"
"#,
            browser.display(),
            browser.display()
        ),
    );

    let output = run_with_env(
        &dir,
        &["explain", notes.to_str().unwrap(), "--lines", "1:1"],
        "",
        Some("secret"),
        &[("FAKE_BROWSER_OUTPUT", output_file.to_str().unwrap())],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(output.stdout.is_empty());

    let captured: Value = serde_json::from_str(&fs::read_to_string(&output_file).unwrap()).unwrap();
    let argv = captured["argv"].as_array().unwrap();
    assert_eq!(argv[0].as_str().unwrap(), expected_target);
    assert!(argv[1].as_str().unwrap().starts_with("http://127.0.0.1:"));
}

#[cfg(unix)]
#[test]
fn config_default_viewer_applies_to_qutebrowser() {
    check_config_viewer("qutebrowser", "--target=tab-bg");
}

#[cfg(unix)]
#[test]
fn config_default_viewer_applies_to_firefox() {
    check_config_viewer("firefox", "--new-tab");
}

#[cfg(unix)]
#[test]
fn viewer_flag_overrides_the_config_default() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"model":"test-model","choices":[{"message":{"role":"assistant","content":"Hello"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let notes = temp.path().join("notes.md");
    fs::write(&notes, "alpha beta gamma\n").unwrap();
    let output_file = temp.path().join("browser.json");
    let browser = temp.path().join("fake-browser");
    write_fake_browser(&browser);
    append_viewer_config(
        &dir,
        &format!(
            r#"
[viewer]
default = "firefox"
firefox = "{}"
"#,
            browser.display()
        ),
    );

    let output = run_with_env(
        &dir,
        &[
            "explain",
            notes.to_str().unwrap(),
            "--lines",
            "1:1",
            "--viewer",
            "terminal",
            "--no-stream",
        ],
        "",
        Some("secret"),
        &[("FAKE_BROWSER_OUTPUT", output_file.to_str().unwrap())],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "Hello");
    assert!(!output_file.exists());
}

#[cfg(unix)]
#[test]
fn run_stream_ignores_the_config_default_viewer() {
    let body = concat!(
        "data: {\"model\":\"test-model\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
        "data: [DONE]\n\n"
    );
    let server = MockServer::start(MockResponse::sse(200, body));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let output_file = temp.path().join("browser.json");
    let browser = temp.path().join("fake-browser");
    write_fake_browser(&browser);
    append_viewer_config(
        &dir,
        &format!(
            r#"
[viewer]
default = "firefox"
firefox = "{}"
"#,
            browser.display()
        ),
    );

    let input =
        r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10}"#;
    let output = run_with_env(
        &dir,
        &["run", "--profile", "explain", "--stream"],
        input,
        Some("secret"),
        &[("FAKE_BROWSER_OUTPUT", output_file.to_str().unwrap())],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"type\":\"done\""));
    assert!(!output_file.exists());
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

fn history_database(dir: &Path) -> std::path::PathBuf {
    dir.join("docent.db")
}

#[derive(Debug, PartialEq)]
struct HistoryRow {
    command: String,
    profile: String,
    source: String,
    output_mode: String,
    status: String,
    model: String,
    output: String,
    selection: String,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    error: Option<String>,
}

#[test]
fn run_history_records_a_successful_run() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"model":"test-model","choices":[{"message":{"content":"Hello **world**"}}],"usage":{"prompt_tokens":3,"completion_tokens":4}}"#,
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

    let connection = rusqlite::Connection::open(history_database(&dir)).unwrap();
    let row = connection
        .query_row(
            "SELECT command, profile, source, output_mode, status, model, output, selection, input_tokens, output_tokens, error FROM runs",
            [],
            |row| {
                Ok(HistoryRow {
                    command: row.get(0)?,
                    profile: row.get(1)?,
                    source: row.get(2)?,
                    output_mode: row.get(3)?,
                    status: row.get(4)?,
                    model: row.get(5)?,
                    output: row.get(6)?,
                    selection: row.get(7)?,
                    input_tokens: row.get(8)?,
                    output_tokens: row.get(9)?,
                    error: row.get(10)?,
                })
            },
        )
        .unwrap();

    assert_eq!(
        row,
        HistoryRow {
            command: "run".to_string(),
            profile: "explain".to_string(),
            source: "cli".to_string(),
            output_mode: "json".to_string(),
            status: "success".to_string(),
            model: "test-model".to_string(),
            output: "Hello **world**".to_string(),
            selection: "beta".to_string(),
            input_tokens: Some(3),
            output_tokens: Some(4),
            error: None,
        }
    );
}

#[test]
fn run_history_records_the_source_flag() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"ok"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let input = r#"{"selection":"beta","document":"alpha beta gamma"}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain", "--source", "my-plugin"],
        input,
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let connection = rusqlite::Connection::open(history_database(&dir)).unwrap();
    let source: String = connection
        .query_row("SELECT source FROM runs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(source, "my-plugin");
}

#[test]
fn run_history_records_provider_errors() {
    let server = MockServer::start(MockResponse::json(
        401,
        r#"{"error":{"message":"bad key"}}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let input = r#"{"selection":"beta","document":"alpha beta gamma"}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain"],
        input,
        Some("secret"),
    );
    assert_eq!(output.status.code(), Some(3));

    let connection = rusqlite::Connection::open(history_database(&dir)).unwrap();
    let (status, error, recorded_output): (String, String, String) = connection
        .query_row("SELECT status, error, output FROM runs", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!(status, "error");
    assert!(error.contains("HTTP 401"));
    assert!(error.contains("bad key"));
    assert_eq!(recorded_output, "");
}

#[test]
fn run_history_records_streaming_output() {
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

    let input = r#"{"selection":"beta","document":"alpha beta gamma"}"#;
    let output = run(
        &dir,
        &[
            "run",
            "--profile",
            "explain",
            "--stream",
            "--source",
            "nvim",
        ],
        input,
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let connection = rusqlite::Connection::open(history_database(&dir)).unwrap();
    let (status, source, output_mode, recorded_output, input_tokens, output_tokens): (
        String,
        String,
        String,
        String,
        Option<i64>,
        Option<i64>,
    ) = connection
        .query_row(
            "SELECT status, source, output_mode, output, input_tokens, output_tokens FROM runs",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(status, "success");
    assert_eq!(source, "nvim");
    assert_eq!(output_mode, "ndjson");
    assert_eq!(recorded_output, "Hello world");
    assert_eq!(input_tokens, Some(3));
    assert_eq!(output_tokens, Some(2));
}

#[test]
fn run_history_records_partial_stream_output_on_error() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
        "data: {\"error\":{\"message\":\"boom\"}}\n\n"
    );
    let server = MockServer::start(MockResponse::sse(200, body));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let input = r#"{"selection":"beta","document":"alpha beta gamma"}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain", "--stream"],
        input,
        Some("secret"),
    );
    assert_eq!(output.status.code(), Some(3));

    let connection = rusqlite::Connection::open(history_database(&dir)).unwrap();
    let (status, recorded_output, error): (String, String, String) = connection
        .query_row("SELECT status, output, error FROM runs", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!(status, "error");
    assert_eq!(recorded_output, "Hello");
    assert!(error.contains("boom"));
}

#[test]
fn print_prompt_does_not_write_history() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", "http://127.0.0.1:1");

    let input = r#"{"selection":"beta","document":"alpha beta gamma"}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain", "--print-prompt"],
        input,
        None,
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(!history_database(&dir).exists());
}

#[test]
fn no_history_skips_the_database() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"ok"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let input = r#"{"selection":"beta","document":"alpha beta gamma"}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain", "--no-history"],
        input,
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(!history_database(&dir).exists());
}

#[test]
fn disabled_history_skips_the_database() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"ok"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let config = fs::read_to_string(dir.join("config.toml")).unwrap();
    let config = config.replace("[history]\n", "[history]\nenabled = false\n");
    fs::write(dir.join("config.toml"), config).unwrap();

    let input = r#"{"selection":"beta","document":"alpha beta gamma"}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain"],
        input,
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(!history_database(&dir).exists());
}

#[test]
fn history_failures_do_not_fail_the_run() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"choices":[{"message":{"content":"still works"}}]}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    fs::create_dir(history_database(&dir)).unwrap();

    let input = r#"{"selection":"beta","document":"alpha beta gamma"}"#;
    let output = run(
        &dir,
        &["run", "--profile", "explain"],
        input,
        Some("secret"),
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "still works\n");
    assert!(stderr(&output).contains("could not record run history"));
}

fn completion_response(text: &str) -> MockResponse {
    MockResponse::json(
        200,
        format!(r#"{{"model":"test-model","choices":[{{"message":{{"content":"{text}"}}}}]}}"#),
    )
}

fn cache_rows(dir: &Path) -> i64 {
    let connection = rusqlite::Connection::open(history_database(dir)).unwrap();
    connection
        .query_row("SELECT COUNT(*) FROM cache", [], |row| row.get(0))
        .unwrap()
}

const EXPLAIN_INPUT: &str = r#"{"selection":"beta","document":"alpha beta gamma","byte_start":6,"byte_end":10,"file":"notes.md"}"#;

#[test]
fn cache_reuses_a_successful_answer() {
    let server = MockServer::start(MockResponse::json(
        200,
        r#"{"model":"test-model","choices":[{"message":{"content":"Hello **world**"}}],"usage":{"prompt_tokens":3,"completion_tokens":4}}"#,
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    enable_cache(&dir);

    let first = run(
        &dir,
        &["run", "--profile", "explain", "--json"],
        EXPLAIN_INPUT,
        Some("secret"),
    );
    assert!(first.status.success(), "stderr: {}", stderr(&first));

    let second = run(
        &dir,
        &["run", "--profile", "explain", "--json"],
        EXPLAIN_INPUT,
        None,
    );
    assert!(second.status.success(), "stderr: {}", stderr(&second));

    assert_eq!(server.requests().len(), 1);

    let first_value: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert!(first_value.get("cached").is_none());

    let second_value: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second_value["text"], "Hello **world**");
    assert_eq!(second_value["cached"], true);
    assert!(second_value.get("usage").is_none());

    let connection = rusqlite::Connection::open(history_database(&dir)).unwrap();
    let runs: i64 = connection
        .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
        .unwrap();
    let hits: i64 = connection
        .query_row("SELECT hits FROM cache", [], |row| row.get(0))
        .unwrap();
    assert_eq!(runs, 1);
    assert_eq!(hits, 1);
}

#[test]
fn no_cache_bypasses_reads_and_writes() {
    let server = MockServer::start_many(vec![
        completion_response("first"),
        completion_response("second"),
    ]);
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    enable_cache(&dir);

    let first = run(
        &dir,
        &["run", "--profile", "explain", "--no-cache"],
        EXPLAIN_INPUT,
        Some("secret"),
    );
    assert!(first.status.success(), "stderr: {}", stderr(&first));
    let second = run(
        &dir,
        &["run", "--profile", "explain", "--no-cache"],
        EXPLAIN_INPUT,
        Some("secret"),
    );
    assert!(second.status.success(), "stderr: {}", stderr(&second));

    assert_eq!(server.requests().len(), 2);
    assert_eq!(String::from_utf8_lossy(&first.stdout), "first\n");
    assert_eq!(String::from_utf8_lossy(&second.stdout), "second\n");
    assert_eq!(cache_rows(&dir), 0);
}

#[test]
fn refresh_replaces_a_stored_answer() {
    let server = MockServer::start_many(vec![
        completion_response("first"),
        completion_response("second"),
    ]);
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    enable_cache(&dir);

    let first = run(
        &dir,
        &["run", "--profile", "explain"],
        EXPLAIN_INPUT,
        Some("secret"),
    );
    assert!(first.status.success(), "stderr: {}", stderr(&first));
    let refreshed = run(
        &dir,
        &["run", "--profile", "explain", "--refresh"],
        EXPLAIN_INPUT,
        Some("secret"),
    );
    assert!(refreshed.status.success(), "stderr: {}", stderr(&refreshed));
    let third = run(&dir, &["run", "--profile", "explain"], EXPLAIN_INPUT, None);
    assert!(third.status.success(), "stderr: {}", stderr(&third));

    assert_eq!(server.requests().len(), 2);
    assert_eq!(String::from_utf8_lossy(&first.stdout), "first\n");
    assert_eq!(String::from_utf8_lossy(&refreshed.stdout), "second\n");
    assert_eq!(String::from_utf8_lossy(&third.stdout), "second\n");
}

#[test]
fn disabled_cache_calls_the_provider_every_time() {
    let server = MockServer::start_many(vec![
        completion_response("first"),
        completion_response("second"),
    ]);
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());

    let first = run(
        &dir,
        &["run", "--profile", "explain"],
        EXPLAIN_INPUT,
        Some("secret"),
    );
    assert!(first.status.success(), "stderr: {}", stderr(&first));
    let second = run(
        &dir,
        &["run", "--profile", "explain"],
        EXPLAIN_INPUT,
        Some("secret"),
    );
    assert!(second.status.success(), "stderr: {}", stderr(&second));

    assert_eq!(server.requests().len(), 2);
    assert_eq!(cache_rows(&dir), 0);
}

#[test]
fn no_history_disables_the_cache() {
    let server = MockServer::start_many(vec![
        completion_response("first"),
        completion_response("second"),
    ]);
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    enable_cache(&dir);

    for _ in 0..2 {
        let output = run(
            &dir,
            &["run", "--profile", "explain", "--no-history"],
            EXPLAIN_INPUT,
            Some("secret"),
        );
        assert!(output.status.success(), "stderr: {}", stderr(&output));
    }

    assert_eq!(server.requests().len(), 2);
    assert!(!history_database(&dir).exists());
}

#[test]
fn errors_are_not_cached() {
    let server = MockServer::start_many(vec![
        MockResponse::json(401, r#"{"error":{"message":"bad key"}}"#),
        completion_response("recovered"),
    ]);
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    enable_cache(&dir);

    let failed = run(
        &dir,
        &["run", "--profile", "explain"],
        EXPLAIN_INPUT,
        Some("secret"),
    );
    assert_eq!(failed.status.code(), Some(3));
    let recovered = run(
        &dir,
        &["run", "--profile", "explain"],
        EXPLAIN_INPUT,
        Some("secret"),
    );
    assert!(recovered.status.success(), "stderr: {}", stderr(&recovered));

    assert_eq!(server.requests().len(), 2);
    assert_eq!(String::from_utf8_lossy(&recovered.stdout), "recovered\n");
}

#[test]
fn ndjson_replays_a_cached_answer() {
    let server = MockServer::start(MockResponse::sse(
        200,
        [
            "data: {\"model\":\"test-model\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
            "data: [DONE]\n\n",
        ]
        .concat(),
    ));
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    enable_cache(&dir);

    let first = run(
        &dir,
        &["run", "--profile", "explain", "--stream"],
        EXPLAIN_INPUT,
        Some("secret"),
    );
    assert!(first.status.success(), "stderr: {}", stderr(&first));

    let second = run(
        &dir,
        &["run", "--profile", "explain", "--stream"],
        EXPLAIN_INPUT,
        None,
    );
    assert!(second.status.success(), "stderr: {}", stderr(&second));

    assert_eq!(server.requests().len(), 1);

    let events: Vec<Value> = String::from_utf8_lossy(&second.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["type"], "delta");
    assert_eq!(events[0]["text"], "Hello world");
    assert_eq!(events[1]["type"], "done");
    assert_eq!(events[1]["cached"], true);
}

#[test]
fn changing_the_document_invalidates_the_cache() {
    let server = MockServer::start_many(vec![
        completion_response("first"),
        completion_response("second"),
    ]);
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    enable_cache(&dir);

    let first = run(
        &dir,
        &["run", "--profile", "explain"],
        EXPLAIN_INPUT,
        Some("secret"),
    );
    assert!(first.status.success(), "stderr: {}", stderr(&first));
    let second = run(
        &dir,
        &["run", "--profile", "explain"],
        r#"{"selection":"beta","document":"alpha beta gamma delta","byte_start":6,"byte_end":10,"file":"notes.md"}"#,
        Some("secret"),
    );
    assert!(second.status.success(), "stderr: {}", stderr(&second));

    assert_eq!(server.requests().len(), 2);
    assert_eq!(String::from_utf8_lossy(&second.stdout), "second\n");
}

#[test]
fn changing_a_parameter_invalidates_the_cache() {
    let server = MockServer::start_many(vec![
        completion_response("first"),
        completion_response("second"),
    ]);
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    setup(&dir, "openai", &server.base_url());
    enable_cache(&dir);

    let first = run(
        &dir,
        &["run", "--profile", "explain"],
        EXPLAIN_INPUT,
        Some("secret"),
    );
    assert!(first.status.success(), "stderr: {}", stderr(&first));

    let path = dir.join("config.toml");
    let contents = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        contents.replace("temperature = 0.2", "temperature = 0.4"),
    )
    .unwrap();

    let second = run(
        &dir,
        &["run", "--profile", "explain"],
        EXPLAIN_INPUT,
        Some("secret"),
    );
    assert!(second.status.success(), "stderr: {}", stderr(&second));

    assert_eq!(server.requests().len(), 2);
    assert_eq!(String::from_utf8_lossy(&second.stdout), "second\n");
}
