//! Loopback HTTP server used by the qutebrowser answer viewer.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;

use crate::error::DocentError;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const KEEPALIVE: Duration = Duration::from_secs(15);

/// One event sent to the answer page over Server-Sent Events.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ServerEvent {
    /// A streamed text fragment.
    Delta { text: String },
    /// The answer rendered as a safe HTML fragment.
    Html { html: String },
    /// The stream finished successfully.
    Done { label: String },
    /// The stream failed.
    Error { message: String },
}

impl ServerEvent {
    /// True for the events that end the stream.
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Done { .. } | Self::Error { .. })
    }
}

struct Shared {
    token: String,
    page: String,
    events: Mutex<Option<Receiver<ServerEvent>>>,
    connected: mpsc::Sender<()>,
    finished: mpsc::Sender<()>,
    cancel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

/// One running viewer server.
pub(super) struct Server {
    path: String,
    addr: SocketAddr,
    sender: SyncSender<ServerEvent>,
    connected: Receiver<()>,
    finished: Receiver<()>,
    cancel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl Server {
    /// Bind a loopback port and start serving the prebuilt page.
    pub(super) fn start(page: String) -> Result<Self, DocentError> {
        let token = random_token()?;
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
            DocentError::Viewer(format!("could not bind the answer server: {error}"))
        })?;
        listener.set_nonblocking(true).map_err(|error| {
            DocentError::Viewer(format!("could not configure the answer server: {error}"))
        })?;
        let addr = listener.local_addr().map_err(|error| {
            DocentError::Viewer(format!("could not read the answer server address: {error}"))
        })?;
        let path = format!("/{token}/");

        let (sender, receiver) = mpsc::sync_channel(0);
        let (connected_tx, connected_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));

        let shared = Arc::new(Shared {
            token,
            page,
            events: Mutex::new(Some(receiver)),
            connected: connected_tx,
            finished: finished_tx,
            cancel: Arc::clone(&cancel),
            stop: Arc::clone(&stop),
        });
        let accept = thread::spawn(move || accept_loop(&listener, &shared));

        Ok(Self {
            path,
            addr,
            sender,
            connected: connected_rx,
            finished: finished_rx,
            cancel,
            stop,
            accept: Some(accept),
        })
    }

    /// URL the browser should open.
    pub(super) fn url(&self) -> String {
        format!("http://127.0.0.1:{}{}", self.addr.port(), self.path)
    }

    /// Send one event to the connected page.
    pub(super) fn send(&self, event: ServerEvent) -> Result<(), DocentError> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(DocentError::ViewerClosed);
        }
        self.sender
            .send(event)
            .map_err(|_| DocentError::ViewerClosed)
    }

    /// Wait until the page opens its event stream.
    pub(super) fn wait_connected(&self, timeout: Duration) -> bool {
        self.connected.recv_timeout(timeout).is_ok()
    }

    /// Wait until the event stream has been written and closed.
    pub(super) fn wait_finished(&self, timeout: Duration) {
        let _ = self.finished.recv_timeout(timeout);
    }

    /// Stop accepting connections and wake any blocked sender.
    pub(super) fn stop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
    }
}

fn accept_loop(listener: &TcpListener, shared: &Arc<Shared>) {
    while !shared.stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let shared = Arc::clone(shared);
                thread::spawn(move || {
                    let _ = handle_connection(stream, &shared);
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

fn handle_connection(mut stream: TcpStream, shared: &Shared) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(CONNECT_TIMEOUT))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    if request_line.len() > 8192 {
        return respond(&mut stream, "400 Bad Request", "text/plain", b"");
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    if method != "GET" {
        return respond(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain",
            b"method not allowed",
        );
    }

    let mut headers = 0;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        headers += read;
        if read == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if headers > 16 * 1024 {
            return Ok(());
        }
    }

    let path = target.split('?').next().unwrap_or(target);
    let events_path = format!("{}events", shared.token_path());
    if path == shared.token_path() {
        respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            shared.page.as_bytes(),
        )
    } else if path == events_path {
        send_events(stream, shared)
    } else {
        respond(&mut stream, "404 Not Found", "text/plain", b"")
    }
}

impl Shared {
    fn token_path(&self) -> String {
        format!("/{}/", self.token)
    }
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

fn send_events(mut stream: TcpStream, shared: &Shared) -> io::Result<()> {
    let events = match shared.events.lock() {
        Ok(mut events) => events.take(),
        Err(_) => None,
    };
    let Some(events) = events else {
        return respond(
            &mut stream,
            "409 Conflict",
            "text/plain",
            b"already connected",
        );
    };

    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nTransfer-Encoding: chunked\r\n\r\n",
    )?;
    stream.flush()?;
    let _ = shared.connected.send(());

    loop {
        let event = match events.recv_timeout(KEEPALIVE) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => {
                if write_chunk(&mut stream, b": keepalive\n\n").is_err() {
                    shared.cancel.store(true, Ordering::Relaxed);
                    break;
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };

        let terminal = event.is_terminal();
        let json = serde_json::to_string(&event)
            .unwrap_or_else(|_| r#"{"type":"error","message":"serialization failed"}"#.to_string());
        if write_chunk(&mut stream, format!("data: {json}\n\n").as_bytes()).is_err() {
            shared.cancel.store(true, Ordering::Relaxed);
            break;
        }
        if terminal {
            break;
        }
    }

    let _ = write_chunk(&mut stream, b"");
    let _ = stream.flush();
    let _ = shared.finished.send(());
    Ok(())
}

fn write_chunk(stream: &mut TcpStream, data: &[u8]) -> io::Result<()> {
    if data.is_empty() {
        stream.write_all(b"0\r\n\r\n")?;
    } else {
        write!(stream, "{:X}\r\n", data.len())?;
        stream.write_all(data)?;
        stream.write_all(b"\r\n")?;
    }
    stream.flush()
}

fn random_token() -> Result<String, DocentError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| {
        DocentError::Viewer(format!("could not generate an answer page token: {error}"))
    })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn start(page: &str) -> (Server, String) {
        let server = Server::start(page.to_string()).unwrap();
        let path = server.path.clone();
        (server, path)
    }

    fn request(server: &Server, path: &str) -> String {
        let mut stream = TcpStream::connect(server.addr).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn serves_the_page_at_the_token_path() {
        let (mut server, path) = start("<html>answer</html>");
        let response = request(&server, &path);
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("text/html; charset=utf-8"));
        assert!(response.contains("<html>answer</html>"));
        server.stop();
    }

    #[test]
    fn rejects_unknown_paths() {
        let (mut server, _) = start("page");
        let response = request(&server, "/not-the-token/");
        assert!(response.starts_with("HTTP/1.1 404 Not Found"));
        server.stop();
    }

    #[test]
    fn tokens_differ_between_servers() {
        let (mut first, first_path) = start("page");
        let (mut second, second_path) = start("page");
        assert_ne!(first_path, second_path);
        first.stop();
        second.stop();
    }

    #[test]
    fn streams_deltas_html_and_done() {
        let (mut server, _) = start("page");
        let addr = server.addr;
        let path = server.path.clone();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).unwrap();
            write!(
                stream,
                "GET {path}events HTTP/1.1\r\nHost: localhost\r\n\r\n"
            )
            .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        });

        assert!(server.wait_connected(Duration::from_secs(2)));
        server
            .send(ServerEvent::Delta {
                text: "Hello".to_string(),
            })
            .unwrap();
        server
            .send(ServerEvent::Html {
                html: "<p>Hello</p>".to_string(),
            })
            .unwrap();
        server
            .send(ServerEvent::Done {
                label: "docent: explain".to_string(),
            })
            .unwrap();
        server.wait_finished(Duration::from_secs(2));

        let response = client.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("text/event-stream"));
        assert!(response.contains(r#"data: {"type":"delta","text":"Hello"}"#));
        assert!(response.contains(r#"data: {"type":"html","html":"<p>Hello</p>"}"#));
        assert!(response.contains(r#"data: {"type":"done","label":"docent: explain"}"#));
        server.stop();
    }

    #[test]
    fn sending_after_stop_reports_a_closed_page() {
        let (mut server, _) = start("page");
        server.stop();
        let error = server
            .send(ServerEvent::Delta {
                text: "late".to_string(),
            })
            .unwrap_err();
        assert!(matches!(error, DocentError::ViewerClosed));
    }
}
