//! Qutebrowser and Firefox answer viewers backed by a loopback Server-Sent
//! Events server.

mod page;
mod server;

use std::process::{Command, Stdio};
use std::time::Duration;

use clap::ValueEnum;

use crate::error::DocentError;
use crate::markdown;
use crate::provider::Completion;
use server::{Server, ServerEvent};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const FINISH_TIMEOUT: Duration = Duration::from_secs(5);

/// A qutebrowser `--target` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum QutebrowserTarget {
    /// Use qutebrowser's configured new-instance target.
    Auto,
    /// Open a new tab.
    Tab,
    /// Open a background tab.
    TabBg,
    /// Open a tab without activating the window.
    TabSilent,
    /// Open a background tab without activating the window.
    TabBgSilent,
    /// Open a new window.
    Window,
    /// Open a private window.
    PrivateWindow,
}

impl QutebrowserTarget {
    /// Value passed as `qutebrowser --target=...`.
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Tab => "tab",
            Self::TabBg => "tab-bg",
            Self::TabSilent => "tab-silent",
            Self::TabBgSilent => "tab-bg-silent",
            Self::Window => "window",
            Self::PrivateWindow => "private-window",
        }
    }
}

/// A Firefox URL target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FirefoxTarget {
    /// Let Firefox use its normal new-instance behavior.
    Auto,
    /// Open a new tab.
    Tab,
    /// Open a new window.
    Window,
    /// Open a new private window.
    PrivateWindow,
}

impl FirefoxTarget {
    /// Arguments that open `url` in this target.
    fn arguments(self) -> &'static [&'static str] {
        match self {
            Self::Auto => &[],
            Self::Tab => &["--new-tab"],
            Self::Window => &["--new-window"],
            Self::PrivateWindow => &["--private-window"],
        }
    }
}

/// Browser program and target used by the answer viewer.
#[derive(Debug, Clone)]
pub enum Browser {
    /// Qutebrowser with its `--target` flag.
    Qutebrowser {
        /// Executable or command name.
        command: String,
        /// Qutebrowser target.
        target: QutebrowserTarget,
    },
    /// Firefox with its URL flags.
    Firefox {
        /// Executable or command name.
        command: String,
        /// Firefox target.
        target: FirefoxTarget,
    },
}

impl Browser {
    /// Name used in user-facing errors.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Qutebrowser { .. } => "qutebrowser",
            Self::Firefox { .. } => "Firefox",
        }
    }

    /// Target flag named in a connection timeout error.
    pub fn target_flag(&self) -> &'static str {
        match self {
            Self::Qutebrowser { .. } => "--qutebrowser-target",
            Self::Firefox { .. } => "--firefox-target",
        }
    }

    fn command(&self) -> &str {
        match self {
            Self::Qutebrowser { command, .. } | Self::Firefox { command, .. } => command,
        }
    }

    fn arguments(&self) -> Vec<String> {
        match self {
            Self::Qutebrowser { target, .. } => {
                vec![format!("--target={}", target.as_str())]
            }
            Self::Firefox { target, .. } => target
                .arguments()
                .iter()
                .map(|arg| (*arg).to_string())
                .collect(),
        }
    }

    /// Command and arguments, including the answer page URL.
    fn launch(&self, url: &str) -> (String, Vec<String>) {
        let mut arguments = self.arguments();
        arguments.push(url.to_string());
        (self.command().to_string(), arguments)
    }
}

/// Settings for one browser answer page.
#[derive(Debug, Clone)]
pub struct ViewerOptions {
    /// Profile shown in the header and title.
    pub profile: String,
    /// File or page label shown after the profile. May be empty.
    pub header: String,
    /// Browser and target.
    pub browser: Browser,
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
    /// Start the server, open the browser, and wait for the page to connect.
    pub fn open(options: &ViewerOptions) -> Result<Self, DocentError> {
        require_graphical_session(options.browser.name())?;
        let page = page::build_page(
            &options.profile,
            &options.header,
            options.custom_css.as_deref(),
        );
        let mut server = Server::start(page)?;
        let (command, arguments) = options.browser.launch(&server.url());

        let mut child = match Command::new(&command)
            .args(&arguments)
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
                    options.browser.name()
                )));
            }
        };

        if !server.wait_connected(CONNECT_TIMEOUT) {
            let _ = child.kill();
            let _ = child.wait();
            server.stop();
            return Err(DocentError::Viewer(format!(
                "{} did not open the answer page; check {} and the {} process",
                options.browser.name(),
                options.browser.target_flag(),
                options.browser.name()
            )));
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

/// Rejects a browser viewer when an X11 or Wayland session has no display.
#[cfg(all(unix, not(target_os = "macos")))]
fn require_graphical_session(browser: &str) -> Result<(), DocentError> {
    check_graphical_session(
        browser,
        std::env::var_os("DISPLAY").is_some(),
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
    )
}

/// macOS and Windows have no X11 or Wayland variables, so the viewer is allowed.
#[cfg(any(windows, target_os = "macos"))]
fn require_graphical_session(_browser: &str) -> Result<(), DocentError> {
    Ok(())
}

/// Checks the display variables of an X11 or Wayland session.
#[cfg(all(unix, not(target_os = "macos")))]
fn check_graphical_session(browser: &str, display: bool, wayland: bool) -> Result<(), DocentError> {
    if !display && !wayland {
        return Err(DocentError::Viewer(format!(
            "no graphical session; {browser} answer pages need DISPLAY or WAYLAND_DISPLAY"
        )));
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

    #[test]
    fn qutebrowser_launch_uses_the_target_flag() {
        let browser = Browser::Qutebrowser {
            command: "qutebrowser".to_string(),
            target: QutebrowserTarget::TabBg,
        };
        assert_eq!(
            browser.launch("http://127.0.0.1:1/token/"),
            (
                "qutebrowser".to_string(),
                vec![
                    "--target=tab-bg".to_string(),
                    "http://127.0.0.1:1/token/".to_string()
                ]
            )
        );
        assert_eq!(browser.name(), "qutebrowser");
        assert_eq!(browser.target_flag(), "--qutebrowser-target");
    }

    #[test]
    fn firefox_launch_maps_every_target() {
        let launch = |target| {
            Browser::Firefox {
                command: "/usr/bin/firefox".to_string(),
                target,
            }
            .launch("http://127.0.0.1:1/token/")
        };

        assert_eq!(
            launch(FirefoxTarget::Auto),
            (
                "/usr/bin/firefox".to_string(),
                vec!["http://127.0.0.1:1/token/".to_string()]
            )
        );
        assert_eq!(
            launch(FirefoxTarget::Tab).1,
            vec![
                "--new-tab".to_string(),
                "http://127.0.0.1:1/token/".to_string()
            ]
        );
        assert_eq!(
            launch(FirefoxTarget::Window).1,
            vec![
                "--new-window".to_string(),
                "http://127.0.0.1:1/token/".to_string()
            ]
        );
        assert_eq!(
            launch(FirefoxTarget::PrivateWindow).1,
            vec![
                "--private-window".to_string(),
                "http://127.0.0.1:1/token/".to_string()
            ]
        );
        assert_eq!(
            Browser::Firefox {
                command: "firefox".to_string(),
                target: FirefoxTarget::Window
            }
            .name(),
            "Firefox"
        );
        assert_eq!(
            Browser::Firefox {
                command: "firefox".to_string(),
                target: FirefoxTarget::Window
            }
            .target_flag(),
            "--firefox-target"
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn requires_a_display_or_wayland_session() {
        let error = check_graphical_session("qutebrowser", false, false).unwrap_err();
        assert!(error.to_string().contains("no graphical session"));
        assert!(check_graphical_session("qutebrowser", true, false).is_ok());
        assert!(check_graphical_session("qutebrowser", false, true).is_ok());
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn allows_browser_viewers_without_x11_variables() {
        assert!(require_graphical_session("qutebrowser").is_ok());
        assert!(require_graphical_session("Firefox").is_ok());
    }
}
