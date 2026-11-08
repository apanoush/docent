#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Canned HTTP response served by [`MockServer`].
#[derive(Clone, Debug)]
pub struct MockResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response body.
    pub body: String,
    /// Response content type.
    pub content_type: String,
}

impl MockResponse {
    /// Creates a JSON response.
    pub fn json(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
            content_type: "application/json".to_string(),
        }
    }

    /// Creates a Server-Sent Events response.
    pub fn sse(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
            content_type: "text/event-stream".to_string(),
        }
    }
}

/// Request captured by [`MockServer`].
#[derive(Clone, Debug)]
pub struct RecordedRequest {
    /// HTTP method.
    pub method: String,
    /// Request path.
    pub path: String,
    /// Lowercased headers.
    pub headers: Vec<(String, String)>,
    /// Request body.
    pub body: String,
}

impl RecordedRequest {
    /// Returns a header value by lowercased name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_str())
    }

    /// Parses the request body as JSON.
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("request body should be valid JSON")
    }
}

/// One-shot HTTP server used to exercise provider clients without network access.
pub struct MockServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MockServer {
    /// Starts a server that answers one request with `response`.
    pub fn start(response: MockResponse) -> Self {
        Self::start_many(vec![response])
    }

    /// Starts a server that answers subsequent requests with `responses`.
    pub fn start_many(responses: Vec<MockResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let shared = Arc::clone(&requests);
        let shutdown = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&shutdown);

        let handle = thread::spawn(move || {
            for response in responses {
                loop {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    match listener.accept() {
                        Ok((stream, _)) => {
                            handle_request(stream, &response, &shared);
                            break;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => return,
                    }
                }
            }
        });

        Self {
            addr,
            requests,
            shutdown,
            handle: Some(handle),
        }
    }

    /// Base URL to put in a test config.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Requests captured so far.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn handle_request(
    mut stream: TcpStream,
    response: &MockResponse,
    requests: &Arc<Mutex<Vec<RecordedRequest>>>,
) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());

    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }

    let content_length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).unwrap();

    requests.lock().unwrap().push(RecordedRequest {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    });

    let body = response.body.as_bytes();
    let head = format!(
        "HTTP/1.1 {} {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        response.status,
        status_text(response.status),
        response.content_type,
        body.len()
    );
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    stream.flush().unwrap();
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        401 => "Unauthorized",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}
