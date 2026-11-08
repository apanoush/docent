//! Qutebrowser answer viewer backed by a loopback Server-Sent Events server.

mod page;
mod server;

use std::process::{Command, Stdio};
use std::time::Duration;

use crate::error::DocentError;
use crate::markdown;
use crate::provider::Completion;
use server::{Server, ServerEvent};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const FINISH_TIMEOUT: Duration = Duration::from_secs(5);

/// Settings for one browser answer page.
#[derive(Debug, Clone)]
pub struct ViewerOptions {
    /// Profile shown in the header and title.
    pub profile: String,
    /// File or page label shown after the profile. May be empty.
    pub header: String,
    /// Qutebrowser executable or command name.
    pub qutebrowser: String,
    /// Qutebrowser `--target` value.
    pub target: String,
    /// CSS appended after the built-in answer styles.
    pub custom_css: Option<String>,
}

/// One open browser answer page.
pub struct Viewer {
    server: Server,
    profile: String,
    header: String,
}

impl Viewer {
    /// Start the server, open qutebrowser, and wait for the page to connect.
    pub fn open(options: &ViewerOptions) -> Result<Self, DocentError> {
        require_graphical_session()?;
        let page = page::build_page(
            &options.profile,
            &options.header,
            options.custom_css.as_deref(),
        );
        let mut server = Server::start(page)?;

        let mut child = match Command::new(&options.qutebrowser)
            .arg(format!("--target={}", options.target))
            .arg(server.url())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                server.stop();
                return Err(DocentError::Viewer(format!(
                    "could not start {}: {error}",
                    options.qutebrowser
                )));
            }
        };

        if !server.wait_connected(CONNECT_TIMEOUT) {
            let _ = child.kill();
            let _ = child.wait();
            server.stop();
            return Err(DocentError::Viewer(
                "qutebrowser did not open the answer page; check --qutebrowser-target and the qutebrowser process"
                    .to_string(),
            ));
        }

        Ok(Self {
            server,
            profile: options.profile.clone(),
            header: options.header.clone(),
        })
    }

    /// Send one streamed text fragment to the page.
    pub fn send_delta(&self, text: &str) -> Result<(), DocentError> {
        self.server.send(ServerEvent::Delta {
            text: text.to_string(),
        })
    }

    /// Send the final answer or error, then shut the server down.
    pub fn finish(&mut self, result: &Result<Completion, DocentError>) {
        match result {
            Ok(completion) => {
                let _ = self.server.send(ServerEvent::Html {
                    html: markdown::render_answer(&completion.text),
                });
                let _ = self.server.send(ServerEvent::Done {
                    label: self.label(completion.model.as_deref()),
                });
            }
            Err(DocentError::ViewerClosed) => {}
            Err(error) => {
                let _ = self.server.send(ServerEvent::Error {
                    message: error.to_string(),
                });
            }
        }
        self.server.wait_finished(FINISH_TIMEOUT);
        self.server.stop();
    }

    fn label(&self, model: Option<&str>) -> String {
        make_label(&self.profile, &self.header, model)
    }
}

fn make_label(profile: &str, header: &str, model: Option<&str>) -> String {
    let mut label = format!("docent: {profile}");
    if let Some(model) = model {
        label = format!("{label} - {model}");
    }
    if !header.is_empty() {
        label = format!("{label} - {header}");
    }
    label
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.server.stop();
    }
}

fn require_graphical_session() -> Result<(), DocentError> {
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Err(DocentError::Viewer(
            "no graphical session; --viewer qutebrowser needs DISPLAY or WAYLAND_DISPLAY"
                .to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_includes_model_and_header() {
        assert_eq!(
            make_label("explain", "notes.md", Some("test-model")),
            "docent: explain - test-model - notes.md"
        );
        assert_eq!(
            make_label("explain", "notes.md", None),
            "docent: explain - notes.md"
        );
        assert_eq!(make_label("explain", "", None), "docent: explain");
    }
}
