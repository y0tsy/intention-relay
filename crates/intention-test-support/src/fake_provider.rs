//! Fake OpenAI-compatible provider fixtures for real-daemon end-to-end suites.
//!
//! [`FakeProvider`] serves its scripted SSE rounds over plain HTTP on its own
//! loopback port, so a real daemon process configured against
//! [`fixture_config_document`] executes the production provider path without a
//! network dependency.

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
/// The first request receives a tool-call round for the tool the fixture named
/// (`read` unless it names another), carrying the tool arguments the fixture
/// supplied, the second (whose body carries the tool result) receives a text
/// round, and any further request receives an HTTP 500 and is counted as excess
/// traffic. Either round may stream the thinking channel before the content it
/// precedes.
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
        Self::scripted("read", tool_arguments, &[], &[])
    }

    /// Starts a fake provider whose tool round calls `tool_name` with
    /// `tool_arguments` and streams `reasoning` chunks before that call.
    ///
    /// # Panics
    ///
    /// Panics when the loopback listener or the provider thread cannot start.
    #[must_use]
    pub fn start_tool_round(tool_name: &str, tool_arguments: &str, reasoning: &[&str]) -> Self {
        Self::scripted(tool_name, tool_arguments, reasoning, &[])
    }

    /// Starts a fake provider whose text round streams `reasoning` chunks
    /// before the answer it informs.
    ///
    /// # Panics
    ///
    /// Panics when the loopback listener or the provider thread cannot start.
    #[must_use]
    pub fn start_with_reasoning(tool_arguments: &str, reasoning: &[&str]) -> Self {
        Self::scripted("read", tool_arguments, &[], reasoning)
    }

    /// Starts a fake provider whose tool round streams `reasoning` chunks
    /// before the tool call it emits.
    ///
    /// # Panics
    ///
    /// Panics when the loopback listener or the provider thread cannot start.
    #[must_use]
    pub fn start_with_round_reasoning(tool_arguments: &str, reasoning: &[&str]) -> Self {
        Self::script("read", tool_arguments, reasoning, &[], 1, true)
    }

    /// Starts a fake provider that serves `rounds` tool-call rounds and one
    /// final text round.
    ///
    /// The rounds are what let one scripted run commit more rows than a
    /// subscription retains: every round commits its call's row and that call's
    /// result row, so a run of `rounds` rounds commits `rounds * 2` tool rows.
    /// Only the first request is paced, because the subscription the run opens
    /// before its first round observes every later round.
    ///
    /// # Panics
    ///
    /// Panics when the loopback listener or the provider thread cannot start.
    #[must_use]
    pub fn start_rounds(tool_name: &str, tool_arguments: &str, rounds: usize) -> Self {
        Self::script(tool_name, tool_arguments, &[], &[], rounds, false)
    }

    /// Starts a fake provider on one loopback port with its own listener thread.
    ///
    /// # Panics
    ///
    /// Panics when the loopback listener or the provider thread cannot start.
    fn scripted(
        tool_name: &str,
        tool_arguments: &str,
        tool_round_reasoning: &[&str],
        reasoning: &[&str],
    ) -> Self {
        Self::script(
            tool_name,
            tool_arguments,
            tool_round_reasoning,
            reasoning,
            1,
            true,
        )
    }

    /// Starts a fake provider serving `rounds` tool rounds and one text round.
    ///
    /// # Panics
    ///
    /// Panics when the loopback listener or the provider thread cannot start.
    fn script(
        tool_name: &str,
        tool_arguments: &str,
        tool_round_reasoning: &[&str],
        reasoning: &[&str],
        rounds: usize,
        pace_every_round: bool,
    ) -> Self {
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
        // Either round streams its thinking channel first and the content it
        // precedes after it, which is the order the provider-neutral stream
        // reports both channels in.
        let reasoning_events = |chunks: &[&str]| -> String {
            chunks
                .iter()
                .map(|chunk| {
                    let body = serde_json::to_string(&serde_json::json!({
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
                    .expect("reasoning chunk serializes");
                    format!("data: {body}\n\n")
                })
                .collect()
        };
        let script = Script {
            tool_name: tool_name.to_owned(),
            tool_arguments: tool_arguments.to_owned(),
            tool_events: reasoning_events(tool_round_reasoning),
            text_events: reasoning_events(reasoning),
            usage_body: serde_json::to_string(&serde_json::json!({
                "id": "chatcmpl-client-e2e-1",
                "object": "chat.completion.chunk",
                "created": 1,
                "model": "fixture-model",
                "choices": [],
                "usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5},
            }))
            .expect("usage chunk serializes"),
            text_body: serde_json::to_string(&serde_json::json!({
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
            .expect("text chunk serializes"),
            tool_rounds: rounds,
            pace_every_round,
        };
        let thread = thread::Builder::new()
            .name("fake-provider".to_owned())
            .spawn(move || {
                listener
                    .set_nonblocking(true)
                    .expect("provider listener is non-blocking");
                while !thread_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            handle_provider_request(
                                stream,
                                &thread_requests,
                                &thread_excess,
                                &script,
                            );
                        }
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

/// How long the fixture paces one scripted request before answering it.
const ROUND_PACE: Duration = Duration::from_millis(500);

/// One scripted provider script: the rounds it serves and how it paces them.
struct Script {
    /// The tool the fixture advertises and calls.
    tool_name: String,
    /// The arguments every scripted call carries.
    tool_arguments: String,
    /// The reasoning events the tool rounds stream before their call.
    tool_events: String,
    /// The reasoning events the text round streams before its answer.
    text_events: String,
    /// The usage chunk every round ends with.
    usage_body: String,
    /// The text chunk the final round answers with.
    text_body: String,
    /// How many tool-call rounds the script serves before its text round.
    tool_rounds: usize,
    /// Whether every request is paced, or only the first one.
    ///
    /// A test's subscriber must attach before a round's committed state is
    /// published, which is why the fixture's short script paces every round. A
    /// long script paces only its first request: the subscription the first
    /// round established observes every later round, and pacing a hundred
    /// rounds would cost a minute.
    pace_every_round: bool,
}

impl Script {
    /// Returns the response one scripted request receives.
    ///
    /// Every tool round carries its own call identity, because a call's durable
    /// evidence is unique per identity.
    fn response(&self, request_number: usize) -> String {
        if request_number <= self.tool_rounds {
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
                            "id": format!("call_{request_number}"),
                            "type": "function",
                            "function": {
                                "name": self.tool_name,
                                "arguments": self.tool_arguments,
                            },
                        }],
                    },
                    "finish_reason": "tool_calls",
                }],
            }))
            .expect("tool chunk serializes");
            return sse_response(&format!(
                "{}data: {tool_body}\n\ndata: {}\n\ndata: [DONE]\n\n",
                self.tool_events, self.usage_body
            ));
        }
        if request_number == self.tool_rounds + 1 {
            return sse_response(&format!(
                "{}data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                self.text_events, self.text_body, self.usage_body
            ));
        }
        excess_response()
    }
}

/// Claims one scripted round for one accepted connection and answers it.
///
/// The connection is the round: the client opens exactly one connection per
/// provider request. The round is claimed once the connection has carried a
/// request head, and it is answered whether or not its body then arrives in
/// full - a round the script never answers is a provider failure, which the run
/// reports as its own failed run, and only the product may reach that verdict.
/// The request and the response each declare their own lengths, so neither peer
/// depends on how the socket ends: the read bound below ends a stalled read, it
/// never ends a round.
///
/// Returning here is the connection's whole teardown, and an explicit one must
/// not come back. The answer above is written but not yet read, so a teardown
/// here races the client's read of it: `shutdown(Both)`, and the close that
/// follows it, answer with a reset on Windows whenever a byte is still unread or
/// still in flight, and a reset discards the response bytes the peer has not
/// consumed. The client then reports the round it was reading as a provider
/// stream that failed, and the run ends failed at a round this script had
/// already answered - a failure the fixture caused itself. It is a race, so it
/// takes whichever round the client's read loses, which is not every round.
///
/// The drop this return performs has nothing to reset for: the body read above
/// consumed exactly the bytes the request declared, so the receive side is
/// empty, and the response declared its own length, so no reader waits on the
/// close to learn that its answer ended. The socket keeps the default linger, so
/// the close sends what the answer queued and then the FIN.
fn handle_provider_request(
    mut stream: TcpStream,
    requests: &AtomicUsize,
    excess: &AtomicUsize,
    script: &Script,
) {
    // The listener is non-blocking so its accept loop can observe `stop`, and
    // Windows hands an accepted connection the listener's non-blocking mode
    // while Unix hands it a blocking one. A non-blocking read here would end in
    // `WouldBlock`, which the head read below cannot tell from a closed peer:
    // the round would be dropped unanswered while its own request bytes were
    // still in the socket, and dropping that socket answers with a reset. The
    // connection is blocking for its whole life; the read bound below is its
    // only bound.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Some(head) = read_request_head(&mut stream) else {
        // A connection that carried no request is not a round: it is dropped
        // unanswered, and the request that never arrived stays unserved.
        return;
    };
    let request_number = requests.fetch_add(1, Ordering::AcqRel) + 1;
    let body = read_request_body(&mut stream, &head);
    // The provider paces each scripted round so the test's subscriber can
    // attach before the committed state for that round is published.
    if script.pace_every_round || request_number == 1 {
        thread::sleep(ROUND_PACE);
    }
    // The assertion is the fixture's own self-check of one request it read, so
    // it only runs for a body that arrived: the answer below is the round's
    // answer either way.
    if request_number == 1
        && let Some(body) = &body
    {
        let body_text = String::from_utf8_lossy(body);
        assert!(
            body_text.contains(r#""tools":["#)
                && body_text.contains(&format!(r#""name":"{}""#, script.tool_name)),
            "the first provider request advertises tools including {}: {body_text}",
            script.tool_name
        );
    }
    if request_number > script.tool_rounds + 1 {
        excess.fetch_add(1, Ordering::AcqRel);
    }
    write_response(&mut stream, &script.response(request_number));
}

fn write_response(stream: &mut TcpStream, response: &str) {
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// Reads one HTTP request head, up to and including its blank line.
///
/// Returns `None` when the connection carried no complete request head within
/// its read bound, so a connection that sent nothing is not a claimed round.
fn read_request_head(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return None,
            Ok(read) => {
                head.extend_from_slice(&buffer[..read]);
                if head.windows(4).any(|window| window == b"\r\n\r\n") {
                    return Some(head);
                }
            }
        }
    }
}

/// Reads the body one request head declares, from the bytes that read carried.
///
/// The bytes that arrived in the same read as the head are preserved. Returns
/// `None` when the declared body did not arrive in full within the connection's
/// read bound: the caller answers the round it already claimed.
fn read_request_body(stream: &mut TcpStream, head: &[u8]) -> Option<Vec<u8>> {
    let end = head.windows(4).position(|window| window == b"\r\n\r\n")?;
    let length = content_length(&head[..end]);
    let mut body = head[end + 4..].to_vec();
    if body.len() < length {
        let mut remaining = vec![0_u8; length - body.len()];
        if stream.read_exact(&mut remaining).is_err() {
            return None;
        }
        body.extend_from_slice(&remaining);
    }
    body.truncate(length);
    Some(body)
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

/// Renders one scripted round as a completely framed HTTP response.
///
/// The body is written in one piece, so its declared length is exact and the
/// response is complete without an end of stream: a client never waits on the
/// close to learn that the answer ended, and no client can hold a connection
/// open past the body it already carries.
fn sse_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn excess_response() -> String {
    let body = r#"{"error":{"message":"unexpected provider request","type":"server_error"}}"#;
    format!(
        "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}
