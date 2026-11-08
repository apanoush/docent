//! Native messaging bridge for the Firefox adapter.
//!
//! Firefox extensions cannot start processes. The extension launches this
//! binary through native messaging and sends one JSON request. For a run, the
//! bridge starts `docent run --stream`, forwards each stdout line as one
//! native message, and reports the exit code. For `exec` and `css`, it runs a
//! short-lived command and returns its output.
//!
//! Messages are a 4-byte native-endian length prefix followed by UTF-8 JSON.
//! The protocol is described in `docs/modules/firefox.md`.

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::json;

use crate::request::Request;

/// Largest request accepted from the extension.
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024 * 1024;

/// Largest stdout or stderr text returned by `exec` and `css`.
const MAX_RESULT_BYTES: usize = 1024 * 1024;

/// A request from the extension.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostRequest {
    /// Run a profile and stream its NDJSON output.
    Run {
        /// Executable followed by any fixed arguments.
        cmd: Vec<String>,
        /// Arguments appended after `cmd`.
        #[serde(default)]
        args: Vec<String>,
        /// Request written to the child's stdin.
        request: Request,
    },
    /// Run a command to completion and return its output.
    Exec {
        /// Executable followed by any fixed arguments.
        cmd: Vec<String>,
        /// Arguments appended after `cmd`.
        #[serde(default)]
        args: Vec<String>,
    },
    /// Return the contents of the user's `answer.css`, when present.
    Css {
        /// Executable followed by any fixed arguments.
        cmd: Vec<String>,
        /// Arguments appended after `cmd`.
        #[serde(default)]
        args: Vec<String>,
    },
}

/// Reads one length-prefixed message, or `None` on a clean end of input.
pub fn read_message(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut header = [0u8; 4];
    let mut filled = 0;
    while filled < header.len() {
        match reader.read(&mut header[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated message header",
                ));
            }
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }

    let length = u32::from_ne_bytes(header) as usize;
    if length > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("message is {length} bytes; the limit is {MAX_MESSAGE_BYTES}"),
        ));
    }

    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload)?;
    Ok(Some(payload))
}

/// Writes one length-prefixed message.
pub fn write_message(writer: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    let length = u32::try_from(payload.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "message does not fit in a 32-bit length prefix",
        )
    })?;
    writer.write_all(&length.to_ne_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}

/// Runs the bridge on the process's standard input and output.
pub fn run() -> io::Result<()> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    let Some(payload) = read_message(&mut stdin)? else {
        return Ok(());
    };

    let request = match serde_json::from_slice::<HostRequest>(&payload) {
        Ok(request) => request,
        Err(error) => {
            send_fail(&mut stdout, format!("invalid request: {error}"))?;
            return Ok(());
        }
    };

    serve(request, &mut stdout)
}

/// Handles one parsed request, writing zero or more messages to `writer`.
pub fn serve(request: HostRequest, writer: &mut impl Write) -> io::Result<()> {
    match request {
        HostRequest::Run { cmd, args, request } => run_stream(&cmd, &args, &request, writer),
        HostRequest::Exec { cmd, args } => run_exec(&cmd, &args, writer),
        HostRequest::Css { cmd, args } => read_css(&cmd, &args, writer),
    }
}

fn run_stream(
    cmd: &[String],
    args: &[String],
    request: &Request,
    writer: &mut impl Write,
) -> io::Result<()> {
    let mut command = match build_command(cmd, args) {
        Ok(command) => command,
        Err(message) => return send_fail(writer, message),
    };
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return send_fail(writer, format!("could not start {}: {error}", cmd[0]));
        }
    };

    let stderr = child
        .stderr
        .take()
        .map(|stderr| std::thread::spawn(move || read_limited(stderr)));
    let child_stdout = child.stdout.take();

    if let Some(mut stdin) = child.stdin.take() {
        match serde_json::to_vec(request) {
            Ok(payload) => {
                if stdin.write_all(&payload).is_err() {
                    let _ = child.kill();
                    let _ = child.wait();
                    return send_fail(writer, "could not write the request to docent".to_string());
                }
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return send_fail(writer, format!("could not encode the request: {error}"));
            }
        }
    }

    let mut disconnected = false;
    if let Some(stdout) = child_stdout {
        let mut reader = BufReader::new(stdout);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            match reader.read_until(b'\n', &mut buffer) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
            if buffer.last() == Some(&b'\n') {
                buffer.pop();
            }
            if buffer.last() == Some(&b'\r') {
                buffer.pop();
            }
            let line = String::from_utf8_lossy(&buffer).into_owned();
            if write_message(
                writer,
                json!({"type": "line", "line": line}).to_string().as_bytes(),
            )
            .is_err()
            {
                disconnected = true;
                let _ = child.kill();
                break;
            }
        }
    }

    let status = child.wait()?;
    let stderr = stderr
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();

    if disconnected {
        return Ok(());
    }

    let code = status.code().unwrap_or(-1);
    write_message(
        writer,
        json!({"type": "exit", "code": code, "stderr": stderr})
            .to_string()
            .as_bytes(),
    )
}

fn run_exec(cmd: &[String], args: &[String], writer: &mut impl Write) -> io::Result<()> {
    let mut command = match build_command(cmd, args) {
        Ok(command) => command,
        Err(message) => return send_fail(writer, message),
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            return send_fail(writer, format!("could not start {}: {error}", cmd[0]));
        }
    };

    let code = output.status.code().unwrap_or(-1);
    write_message(
        writer,
        json!({
            "type": "result",
            "code": code,
            "stdout": clip(&String::from_utf8_lossy(&output.stdout)),
            "stderr": clip(&String::from_utf8_lossy(&output.stderr)),
        })
        .to_string()
        .as_bytes(),
    )
}

fn read_css(cmd: &[String], args: &[String], writer: &mut impl Write) -> io::Result<()> {
    let mut command = match build_command(cmd, args) {
        Ok(command) => command,
        Err(message) => return send_fail(writer, message),
    };
    command
        .arg("config-path")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            return send_fail(writer, format!("could not start {}: {error}", cmd[0]));
        }
    };
    if !output.status.success() {
        return send_result(
            writer,
            output.status.code().unwrap_or(-1),
            "",
            &String::from_utf8_lossy(&output.stderr),
        );
    }

    let config_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if config_dir.is_empty() {
        return send_result(writer, 1, "", "docent config-path returned no path");
    }

    let path = Path::new(&config_dir).join("answer.css");
    match fs::read_to_string(&path) {
        Ok(css) => send_result(writer, 0, &css, ""),
        Err(error) if error.kind() == io::ErrorKind::NotFound => send_result(writer, 0, "", ""),
        Err(error) => send_result(
            writer,
            1,
            "",
            &format!("could not read {}: {error}", path.display()),
        ),
    }
}

fn build_command(cmd: &[String], args: &[String]) -> Result<Command, String> {
    let Some(program) = cmd.first() else {
        return Err("cmd must name the docent executable".to_string());
    };
    if program.is_empty() {
        return Err("cmd must name the docent executable".to_string());
    }

    let name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if !name.starts_with("docent") {
        return Err(format!(
            "refusing to run '{program}': the executable must be named docent"
        ));
    }

    let mut command = Command::new(program);
    command.args(&cmd[1..]).args(args);
    Ok(command)
}

fn send_fail(writer: &mut impl Write, message: impl Into<String>) -> io::Result<()> {
    write_message(
        writer,
        json!({"type": "fail", "message": message.into()})
            .to_string()
            .as_bytes(),
    )
}

fn send_result(writer: &mut impl Write, code: i32, stdout: &str, stderr: &str) -> io::Result<()> {
    write_message(
        writer,
        json!({
            "type": "result",
            "code": code,
            "stdout": clip(stdout),
            "stderr": clip(stderr),
        })
        .to_string()
        .as_bytes(),
    )
}

fn clip(text: &str) -> String {
    if text.len() <= MAX_RESULT_BYTES {
        return text.to_string();
    }
    let mut end = MAX_RESULT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[truncated]", &text[..end])
}

fn read_limited(mut reader: impl Read) -> String {
    let mut kept = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                let room = MAX_RESULT_BYTES.saturating_sub(kept.len());
                if room > 0 {
                    kept.extend_from_slice(&buffer[..room.min(read)]);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&kept).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io;
    use std::io::Cursor;
    use std::path::Path;

    #[test]
    fn framing_round_trips() {
        let mut buffer = Vec::new();
        write_message(&mut buffer, br#"{"type":"exec"}"#).unwrap();
        let mut reader = Cursor::new(buffer);
        let payload = read_message(&mut reader).unwrap().unwrap();
        assert_eq!(payload, br#"{"type":"exec"}"#);
        assert!(read_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn rejects_oversized_messages() {
        let header = ((MAX_MESSAGE_BYTES + 1) as u32).to_ne_bytes();
        let mut reader = Cursor::new(header.to_vec());
        let error = read_message(&mut reader).unwrap_err();
        assert!(error.to_string().contains("limit"));
    }

    #[test]
    fn rejects_a_truncated_header() {
        let mut reader = Cursor::new(vec![1, 2]);
        let error = read_message(&mut reader).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn parses_run_requests() {
        let request: HostRequest = serde_json::from_str(
            r#"{"type":"run","cmd":["docent"],"args":["run"],"request":{"selection":"x"}}"#,
        )
        .unwrap();
        match request {
            HostRequest::Run { cmd, args, request } => {
                assert_eq!(cmd, ["docent"]);
                assert_eq!(args, ["run"]);
                assert_eq!(request.selection, "x");
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_request_types_and_fields() {
        assert!(serde_json::from_str::<HostRequest>(r#"{"type":"other"}"#).is_err());
        assert!(
            serde_json::from_str::<HostRequest>(r#"{"type":"exec","cmd":["docent"],"extra":1}"#)
                .is_err()
        );
    }

    #[test]
    fn refuses_programs_that_are_not_docent() {
        let error = build_command(&["/bin/sh".to_string()], &[]).unwrap_err();
        assert!(error.contains("must be named docent"));
        assert!(build_command(&[], &[]).is_err());
    }

    #[test]
    fn clips_long_text_at_a_character_boundary() {
        let text = "é".repeat(MAX_RESULT_BYTES);
        let clipped = clip(&text);
        assert!(clipped.ends_with("[truncated]"));
        assert!(clipped.len() <= MAX_RESULT_BYTES + "\n[truncated]".len());
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        use std::path::PathBuf;

        fn fake_docent(directory: &Path, script: &str) -> PathBuf {
            let path = directory.join("docent");
            fs::write(&path, script).unwrap();
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions).unwrap();
            path
        }

        fn messages(buffer: &[u8]) -> Vec<serde_json::Value> {
            let mut reader = Cursor::new(buffer);
            let mut values = Vec::new();
            while let Some(payload) = read_message(&mut reader).unwrap() {
                values.push(serde_json::from_slice(&payload).unwrap());
            }
            values
        }

        #[test]
        fn streams_lines_and_reports_the_exit_code() {
            let directory = tempfile::tempdir().unwrap();
            let program = fake_docent(
                directory.path(),
                "#!/bin/sh\ncat >/dev/null\n\
                 printf '%s\\n' '{\"type\":\"delta\",\"text\":\"hi\"}'\n\
                 printf '%s\\n' '{\"type\":\"done\",\"model\":\"m\"}'\n\
                 echo 'warn: test' >&2\n",
            );

            let request = HostRequest::Run {
                cmd: vec![program.display().to_string()],
                args: vec!["run".to_string()],
                request: Request {
                    selection: "x".to_string(),
                    ..Request::default()
                },
            };

            let mut output = Vec::new();
            serve(request, &mut output).unwrap();
            let messages = messages(&output);

            assert_eq!(messages[0]["type"], "line");
            assert_eq!(messages[0]["line"], "{\"type\":\"delta\",\"text\":\"hi\"}");
            assert_eq!(messages[1]["type"], "line");
            let exit = messages.last().unwrap();
            assert_eq!(exit["type"], "exit");
            assert_eq!(exit["code"], 0);
            assert!(exit["stderr"].as_str().unwrap().contains("warn: test"));
        }

        #[test]
        fn forwards_a_final_line_without_a_newline() {
            let directory = tempfile::tempdir().unwrap();
            let program = fake_docent(
                directory.path(),
                "#!/bin/sh\ncat >/dev/null\nprintf '%s' '{\"type\":\"delta\"}'\n",
            );

            let request = HostRequest::Run {
                cmd: vec![program.display().to_string()],
                args: Vec::new(),
                request: Request::default(),
            };

            let mut output = Vec::new();
            serve(request, &mut output).unwrap();
            let messages = messages(&output);
            assert_eq!(messages[0]["line"], "{\"type\":\"delta\"}");
        }

        #[test]
        fn reports_a_process_that_cannot_start() {
            let request = HostRequest::Exec {
                cmd: vec!["/definitely/missing/docent".to_string()],
                args: Vec::new(),
            };
            let mut output = Vec::new();
            serve(request, &mut output).unwrap();
            let messages = messages(&output);
            assert_eq!(messages[0]["type"], "fail");
            assert!(
                messages[0]["message"]
                    .as_str()
                    .unwrap()
                    .contains("could not start")
            );
        }

        #[test]
        fn exec_returns_output_and_status() {
            let directory = tempfile::tempdir().unwrap();
            let program = fake_docent(
                directory.path(),
                "#!/bin/sh\nprintf '{\"profiles\":[]}'\nprintf 'note' >&2\nexit 3\n",
            );
            let request = HostRequest::Exec {
                cmd: vec![program.display().to_string()],
                args: vec!["profiles".to_string(), "--json".to_string()],
            };
            let mut output = Vec::new();
            serve(request, &mut output).unwrap();
            let messages = messages(&output);
            assert_eq!(messages[0]["type"], "result");
            assert_eq!(messages[0]["code"], 3);
            assert_eq!(messages[0]["stdout"], "{\"profiles\":[]}");
            assert_eq!(messages[0]["stderr"], "note");
        }

        #[test]
        fn css_returns_the_config_stylesheet() {
            let directory = tempfile::tempdir().unwrap();
            let config = directory.path().join("config");
            fs::create_dir(&config).unwrap();
            fs::write(config.join("answer.css"), "body { color: red; }").unwrap();
            let script = format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", config.display());
            let program = fake_docent(directory.path(), &script);

            let request = HostRequest::Css {
                cmd: vec![program.display().to_string()],
                args: Vec::new(),
            };
            let mut output = Vec::new();
            serve(request, &mut output).unwrap();
            let messages = messages(&output);
            assert_eq!(messages[0]["type"], "result");
            assert_eq!(messages[0]["code"], 0);
            assert_eq!(messages[0]["stdout"], "body { color: red; }");
        }

        #[test]
        fn css_is_empty_when_the_file_is_missing() {
            let directory = tempfile::tempdir().unwrap();
            let config = directory.path().join("config");
            fs::create_dir(&config).unwrap();
            let script = format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", config.display());
            let program = fake_docent(directory.path(), &script);

            let request = HostRequest::Css {
                cmd: vec![program.display().to_string()],
                args: Vec::new(),
            };
            let mut output = Vec::new();
            serve(request, &mut output).unwrap();
            let messages = messages(&output);
            assert_eq!(messages[0]["code"], 0);
            assert_eq!(messages[0]["stdout"], "");
        }

        #[test]
        fn refuses_a_non_docent_program() {
            let request = HostRequest::Exec {
                cmd: vec!["/bin/sh".to_string()],
                args: Vec::new(),
            };
            let mut output = Vec::new();
            serve(request, &mut output).unwrap();
            let messages = messages(&output);
            assert_eq!(messages[0]["type"], "fail");
        }
    }
}
