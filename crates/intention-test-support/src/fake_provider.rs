//! Fake OpenAI-compatible provider fixtures for real-daemon end-to-end suites.
//!
//! [`FakeProvider`] serves its scripted SSE rounds over plain HTTP on its own
//! loopback port, so a real daemon process configured against
//! [`fixture_config_document`] executes the production provider path without a
//! network dependency.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "the fake provider fixture fails with a precise diagnostic when a scripted round is violated"
)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

/// Renders the fixture daemon configuration document for one fake provider port.
#[must_use]
pub fn fixture_config_document(port: u16, credential: &str) -> String {
    format!(
        "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"fixture-model\"\nendpoint = \"http://127.0.0.1:{port}/v1\"\ncredential = \"{credential}\"\n"
    )
}

/// A fake OpenAI-compatible provider serving two scripted SSE rounds.
///
/// The first request receives a tool-call round for the `read` tool carrying
/// the tool arguments the fixture supplied, the second (whose body carries the
/// tool result) receives a text round, and any further request receives an
/// HTTP 500 and is counted as excess traffic.
///
/// A text round streams the reasoning chunks the fixture scripted before the
/// answer they inform, exactly as a thinking provider does, so the same script
/// drives the reasoning channel and the answer channel of one step.
pub struct FakeProvider {
    port: u16,
    requests: Arc<AtomicUsize>,
    excess: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FakeProvider {
    /// Starts a fake provider on one loopback port with its own listener thread.
    ///
    /// # Panics
    ///
    /// Panics when the loopback listener or the provider thread cannot start.
    #[must_use]
    pub fn start(tool_arguments: &str) -> Self {
        Self::scripted(tool_arguments, &[])
    }

    /// Starts a fake provider whose text round streams `reasoning` chunks
    /// before the answer it informs.
    ///
    /// # Panics
    ///
    /// Panics when the loopback listener or the provider thread cannot start.
    #[must_use]
    pub fn start_with_reasoning(tool_arguments: &str, reasoning: &[&str]) -> Self {
        Self::scripted(tool_arguments, reasoning)
    }

    /// Starts a fake provider on one loopback port with its own listener thread.
    ///
    /// # Panics
    ///
    /// Panics when the loopback listener or the provider thread cannot start.
    fn scripted(tool_arguments: &str, reasoning: &[&str]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("provider binds");
        let port = listener
            .local_addr()
            .expect("provider port is available")
            .port();
        let requests = Arc::new(AtomicUsize::new(0));
        let excess = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_requests = Arc::clone(&requests);
        let thread_excess = Arc::clone(&excess);
        let thread_stop = Arc::clone(&stop);
        let tool_body = serde_json::to_string(&serde_json::json!({
            "id": "chatcmpl-client-e2e-1",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "fixture-model",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "read", "arguments": tool_arguments},
                    }],
                },
                "finish_reason": "tool_calls",
            }],
        }))
        .expect("tool chunk serializes");
        let usage_body = serde_json::to_string(&serde_json::json!({
            "id": "chatcmpl-client-e2e-1",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "fixture-model",
            "choices": [],
            "usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5},
        }))
        .expect("usage chunk serializes");
        let text_body = serde_json::to_string(&serde_json::json!({
            "id": "chatcmpl-client-e2e-2",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "fixture-model",
            "choices": [{
                "index": 0,
                "delta": {"content": "done"},
                "finish_reason": "stop",
            }],
        }))
        .expect("text chunk serializes");
        let reasoning_bodies = reasoning
            .iter()
            .map(|chunk| {
                serde_json::to_string(&serde_json::json!({
                    "id": "chatcmpl-client-e2e-2",
                    "object": "chat.completion.chunk",
                    "created": 1,
                    "model": "fixture-model",
                    "choices": [{
                        "index": 0,
                        "delta": {"reasoning_content": chunk},
                        "finish_reason": null,
                    }],
                }))
                .expect("reasoning chunk serializes")
            })
            .collect::<Vec<_>>();
        let tool_response = sse_response(&format!(
            "data: {tool_body}\n\ndata: {usage_body}\n\ndata: [DONE]\n\n"
        ));
        // The text round streams the thinking channel first and the answer
        // after it, which is the order the provider-neutral stream reports both
        // channels in.
        let text_events = reasoning_bodies
            .iter()
            .map(|body| format!("data: {body}\n\n"))
            .collect::<String>();
        let text_response = sse_response(&format!(
            "{text_events}data: {text_body}\n\ndata: {usage_body}\n\ndata: [DONE]\n\n"
        ));
        let thread = thread::Builder::new()
            .name("fake-provider".to_owned())
            .spawn(move || {
                listener
                    .set_nonblocking(true)
                    .expect("provider listener is non-blocking");
                while !thread_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => handle_provider_request(
                            stream,
                            &thread_requests,
                            &thread_excess,
                            &tool_response,
                            &text_response,
                        ),
                        Err(_) => thread::sleep(Duration::from_millis(10)),
                    }
                }
            })
            .expect("provider thread starts");
        Self {
            port,
            requests,
            excess,
            stop,
            thread: Some(thread),
        }
    }

    /// Returns the loopback port the provider accepts requests on.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Returns how many provider requests the script served.
    #[must_use]
    pub fn request_count(&self) -> usize {
        self.requests.load(Ordering::Acquire)
    }

    /// Returns how many requests the script counted as excess traffic.
    #[must_use]
    pub fn excess_count(&self) -> usize {
        self.excess.load(Ordering::Acquire)
    }

    /// Stops the provider and joins its listener thread.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Reads one HTTP request head and body and answers it from the script.
fn handle_provider_request(
    mut stream: TcpStream,
    requests: &AtomicUsize,
    excess: &AtomicUsize,
    tool_response: &str,
    text_response: &str,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Some(body) = read_request_body(&mut stream) else {
        return;
    };
    // The provider paces each scripted round so the test's subscriber can
    // attach before the committed state for that round is published.
    thread::sleep(Duration::from_millis(500));
    let request_number = requests.fetch_add(1, Ordering::AcqRel) + 1;
    let body_text = String::from_utf8_lossy(&body);
    if request_number == 1 {
        assert!(
            body_text.contains(r#""tools":["#) && body_text.contains(r#""name":"read""#),
            "the first provider request advertises tools including read: {body_text}"
        );
    }
    if request_number <= 2 {
        let response = if body_text.contains("\"role\":\"tool\"") {
            text_response
        } else {
            tool_response
        };
        write_response(&mut stream, response);
    } else {
        excess.fetch_add(1, Ordering::AcqRel);
        write_response(&mut stream, &excess_response());
    }
}

fn write_response(stream: &mut TcpStream, response: &str) {
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// Reads one complete HTTP request body: the head up to its blank line plus
/// exactly the declared Content-Length bytes, preserving any body bytes that
/// arrived in the same read as the head.
fn read_request_body(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return None,
            Ok(read) => {
                head.extend_from_slice(&buffer[..read]);
                if let Some(end) = head.windows(4).position(|window| window == b"\r\n\r\n") {
                    let length = content_length(&head[..end]);
                    let mut body = head.split_off(end + 4);
                    if body.len() < length {
                        let mut remaining = vec![0_u8; length - body.len()];
                        if stream.read_exact(&mut remaining).is_err() {
                            return None;
                        }
                        body.extend_from_slice(&remaining);
                    }
                    body.truncate(length);
                    return Some(body);
                }
            }
            Err(_) => return None,
        }
    }
}

/// Parses the Content-Length header from an HTTP request head.
fn content_length(head: &[u8]) -> usize {
    let head = String::from_utf8_lossy(head);
    for line in head.lines() {
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            return value.trim().parse().unwrap_or(0);
        }
    }
    0
}

fn sse_response(body: &str) -> String {
    format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}")
}

fn excess_response() -> String {
    let body = r#"{"error":{"message":"unexpected provider request","type":"server_error"}}"#;
    format!(
        "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}
