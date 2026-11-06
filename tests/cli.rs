use std::fs;
use std::process::Command;

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
    for name in ["explain", "ask", "translate"] {
        assert!(dir.join(format!("profiles/{name}.toml")).exists());
    }

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
    assert_eq!(names, vec!["ask", "explain", "translate"]);

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
}

#[test]
fn bare_docent_prints_getting_started_examples() {
    let output = docent().output().unwrap();
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("docent explain notes.md --lines 12:20"));
    assert!(stdout.contains("docent ask notes.md"));
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
            .contains("[api]")
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

    fs::write(dir.join("config.toml"), "[api]\nunknown = true\n").unwrap();

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
