//! End-to-end checks for the Firefox native messaging binary.
//!
//! The unit tests in `docent::native_host` cover the protocol helpers. These
//! tests spawn the real binary so the framing, the exit code, and the reply
//! written to stdout are exercised the way Firefox starts it.

#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

fn host() -> Command {
    Command::new(env!("CARGO_BIN_EXE_docent-native-host"))
}

fn fake_docent(directory: &Path, script: &str) -> PathBuf {
    let path = directory.join("docent");
    fs::write(&path, script).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
}

fn frame(value: &Value) -> Vec<u8> {
    let payload = serde_json::to_vec(value).unwrap();
    let mut framed = Vec::with_capacity(4 + payload.len());
    framed.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
    framed.extend_from_slice(&payload);
    framed
}

fn run_host(input: &[u8]) -> Output {
    let mut child = host()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(input).unwrap();
    drop(stdin);
    child.wait_with_output().unwrap()
}

fn messages(buffer: &[u8]) -> Vec<Value> {
    let mut rest = buffer;
    let mut values = Vec::new();
    while rest.len() >= 4 {
        let (header, tail) = rest.split_at(4);
        let length = u32::from_ne_bytes(header.try_into().unwrap()) as usize;
        assert!(tail.len() >= length, "truncated frame");
        let (payload, tail) = tail.split_at(length);
        values.push(serde_json::from_slice(payload).unwrap());
        rest = tail;
    }
    assert!(rest.is_empty(), "trailing bytes after the last frame");
    values
}

#[test]
fn exec_request_returns_the_command_output() {
    let directory = tempfile::tempdir().unwrap();
    let docent = fake_docent(directory.path(), "#!/bin/sh\nprintf 'hello'\n");

    let output = run_host(&frame(&json!({"type": "exec", "cmd": [docent]})));

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let replies = messages(&output.stdout);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["type"], "result");
    assert_eq!(replies[0]["code"], 0);
    assert_eq!(replies[0]["stdout"], "hello");
    assert_eq!(replies[0]["stderr"], "");
}

#[test]
fn invalid_request_is_reported_as_a_failure() {
    let output = run_host(&frame(&json!({"type": "bogus"})));

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let replies = messages(&output.stdout);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["type"], "fail");
    let message = replies[0]["message"].as_str().unwrap();
    assert!(message.starts_with("invalid request:"), "{message}");
}

#[test]
fn missing_request_exits_cleanly() {
    let output = run_host(b"");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
}
