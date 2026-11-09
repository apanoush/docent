mod common;

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use common::sample_pdf;
use serde_json::Value;

fn docent() -> Command {
    Command::new(env!("CARGO_BIN_EXE_docent"))
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn init_creates_defaults_and_profiles_lists_them() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");

    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "init"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(dir.join("config.toml").exists());
    assert!(dir.join("answer.css").exists());
    let css = fs::read_to_string(dir.join("answer.css")).unwrap();
    assert!(css.contains("--docent-font-body"));
    for name in ["explain", "ask", "translate", "summarize"] {
        assert!(dir.join(format!("profiles/{name}.toml")).exists());
    }
    let init_stdout = String::from_utf8(output.stdout).unwrap();
    assert!(init_stdout.contains("answer.css"));

    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "profiles", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let profiles = value["profiles"].as_array().unwrap();
    let names: Vec<_> = profiles
        .iter()
        .map(|profile| profile["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["ask", "explain", "summarize", "translate"]);

    let ask = profiles
        .iter()
        .find(|profile| profile["name"] == "ask")
        .unwrap();
    let requirements = ask["requires"].as_array().unwrap();
    assert!(requirements.iter().any(|item| item == "selection"));
    assert!(requirements.iter().any(|item| item == "question"));
    assert_eq!(
        ask["description"].as_str().unwrap(),
        "Ask a question about the selected passage"
    );
}

#[test]
fn profiles_auto_initializes_when_config_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");

    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "profiles"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(dir.join("config.toml").exists());

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.lines().any(|line| line.starts_with("explain")));
}

#[test]
fn profiles_prints_descriptions_for_humans() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");

    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "init"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "profiles"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("explain: Explain the selected passage in context"));
    assert!(stdout.contains("ask: Ask a question about the selected passage"));
    assert!(stdout.contains("summarize: Summarize the document or the selected passage"));
}

#[test]
fn bare_docent_prints_getting_started_examples() {
    let output = docent().output().unwrap();
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("docent explain notes.md --lines 12:20"));
    assert!(stdout.contains("docent ask notes.md"));
    assert!(stdout.contains("docent summarize paper.md"));
    assert!(stdout.contains("docent --help"));
}

#[test]
fn config_path_respects_environment() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");

    let output = docent()
        .env("DOCENT_CONFIG_DIR", &dir)
        .arg("config-path")
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        dir.to_str().unwrap()
    );
    assert!(!dir.exists());
}

#[test]
fn init_skips_existing_files_and_force_overwrites() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");

    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "init"])
        .output()
        .unwrap();
    assert!(output.status.success());

    fs::write(dir.join("config.toml"), "not toml").unwrap();
    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "init"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(
        fs::read_to_string(dir.join("config.toml")).unwrap(),
        "not toml"
    );

    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "init", "--force"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(
        fs::read_to_string(dir.join("config.toml"))
            .unwrap()
            .contains("[provider]")
    );
}

#[test]
fn profiles_reports_invalid_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");

    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "init"])
        .output()
        .unwrap();
    assert!(output.status.success());

    fs::write(
        dir.join("profiles/explain.toml"),
        "user_prompt = \"{{question}}\"\n",
    )
    .unwrap();
    fs::remove_file(dir.join("profiles/ask.toml")).unwrap();
    fs::remove_file(dir.join("profiles/translate.toml")).unwrap();
    fs::remove_file(dir.join("profiles/summarize.toml")).unwrap();

    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "profiles"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("requires"));
}

#[test]
fn invalid_config_is_reported() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");

    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "init"])
        .output()
        .unwrap();
    assert!(output.status.success());

    fs::write(dir.join("config.toml"), "[provider]\nunknown = true\n").unwrap();

    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "profiles"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("failed to parse config"));
}

#[test]
fn invalid_profile_name_is_reported_by_run_contract() {
    // Profiles are loaded by name in later passes; this pass only exposes the
    // catalog. The picker must not offer names that are unsafe as file stems.
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");

    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "init"])
        .output()
        .unwrap();
    assert!(output.status.success());

    fs::write(
        dir.join("profiles/bad name.toml"),
        "user_prompt = \"hello\"\n",
    )
    .unwrap();

    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "profiles", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let names: Vec<_> = value["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|profile| profile["name"].as_str().unwrap())
        .collect();
    assert!(!names.contains(&"bad name"));
    assert!(stderr(&output).contains("invalid profile name"));
}

#[test]
fn viewer_rejects_no_stream() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");

    for viewer in ["qutebrowser", "firefox"] {
        let output = docent()
            .args([
                "--config-dir",
                dir.to_str().unwrap(),
                "explain",
                "notes.md",
                "--viewer",
                viewer,
                "--no-stream",
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(4));
        assert!(stderr(&output).contains("cannot be combined with --no-stream"));
    }
}

#[test]
fn viewer_config_default_rejects_no_stream() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");
    let output = docent()
        .args(["--config-dir", dir.to_str().unwrap(), "init"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let path = dir.join("config.toml");
    let contents = fs::read_to_string(&path).unwrap();
    let contents = contents.replace("default = \"terminal\"", "default = \"firefox\"");
    assert!(contents.contains("default = \"firefox\""));
    fs::write(&path, contents).unwrap();

    let output = docent()
        .args([
            "--config-dir",
            dir.to_str().unwrap(),
            "explain",
            "notes.md",
            "--no-stream",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("cannot be combined with --no-stream"));
}

#[test]
fn request_viewer_rejects_streaming_flags() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("config");

    for viewer in ["qutebrowser", "firefox"] {
        let output = docent()
            .args([
                "--config-dir",
                dir.to_str().unwrap(),
                "request",
                "--viewer",
                viewer,
                "--stream",
            ])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(4));
        assert!(stderr(&output).contains("browser viewers cannot be combined"));
    }
}

fn run_extract(args: &[&str], stdin: &[u8]) -> std::process::Output {
    let config = tempfile::tempdir().unwrap();
    let mut child = docent()
        .args(["--config-dir", config.path().to_str().unwrap(), "extract"])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn extract_prints_selected_pdf_pages() {
    let temp = tempfile::tempdir().unwrap();
    let paper = temp.path().join("paper.pdf");
    fs::write(
        &paper,
        sample_pdf(&["alpha page", "beta page", "gamma page"]),
    )
    .unwrap();

    let output = run_extract(&[paper.to_str().unwrap(), "--pages", "2:3"], b"");
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("beta page"), "{text}");
    assert!(text.contains("gamma page"), "{text}");
    assert!(!text.contains("alpha page"), "{text}");
}

#[test]
fn extract_clamps_the_end_page() {
    let temp = tempfile::tempdir().unwrap();
    let paper = temp.path().join("paper.pdf");
    fs::write(&paper, sample_pdf(&["alpha page", "beta page"])).unwrap();

    let output = run_extract(&[paper.to_str().unwrap(), "--pages", "2:99"], b"");
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("beta page"), "{text}");
    assert!(!text.contains("alpha page"), "{text}");
}

#[test]
fn extract_prints_every_page_by_default() {
    let temp = tempfile::tempdir().unwrap();
    let paper = temp.path().join("paper.pdf");
    fs::write(&paper, sample_pdf(&["alpha page", "beta page"])).unwrap();

    let output = run_extract(&[paper.to_str().unwrap()], b"");
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("alpha page"), "{text}");
    assert!(text.contains("beta page"), "{text}");
}

#[test]
fn extract_reads_a_pdf_from_stdin() {
    let pdf = sample_pdf(&["from stdin"]);

    let output = run_extract(&["-", "--page", "1"], &pdf);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("from stdin"), "{text}");
}

#[test]
fn extract_prints_text_files_as_is() {
    let temp = tempfile::tempdir().unwrap();
    let notes = temp.path().join("notes.md");
    fs::write(&notes, "alpha\nbeta\n").unwrap();

    let output = run_extract(&[notes.to_str().unwrap()], b"");
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "alpha\nbeta\n");
}

#[test]
fn extract_rejects_a_start_page_past_the_end() {
    let temp = tempfile::tempdir().unwrap();
    let paper = temp.path().join("paper.pdf");
    fs::write(&paper, sample_pdf(&["alpha page"])).unwrap();

    let output = run_extract(&[paper.to_str().unwrap(), "--pages", "2:3"], b"");
    assert_eq!(output.status.code(), Some(4));
    assert!(
        stderr(&output).contains("out of bounds"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn extract_rejects_pages_for_a_text_file() {
    let temp = tempfile::tempdir().unwrap();
    let notes = temp.path().join("notes.md");
    fs::write(&notes, "alpha\n").unwrap();

    let output = run_extract(&[notes.to_str().unwrap(), "--pages", "1"], b"");
    assert_eq!(output.status.code(), Some(4));
    assert!(
        stderr(&output).contains("--pages requires a PDF"),
        "{}",
        stderr(&output)
    );
}
