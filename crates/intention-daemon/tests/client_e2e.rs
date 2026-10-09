//! Client-driven daemon-host end-to-end tests over real IPC.
//!
//! These tests spawn the real `intention-daemon` binary over real local
//! transport, drive it with the real asynchronous client, and execute a real
//! `read` tool through the production model-tool loop against a fake
//! OpenAI-compatible provider. They prove the committed transcript rows, the
//! run status, and live `run.frame` publication, restart the daemon, and prove
//! the same durable run is replayed as current state without re-executing the
//! tool.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Client end-to-end fixtures use assertion conveniences for precise diagnostics."
)]

mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Child;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use common::{spawn_daemon, unique_endpoint, write_config_document};
use intention_client::{
    IntentionClient, ProcessDaemonLauncher, RunStreamClient, RunStreamSubscription,
};
use intention_proto::{
    CreateSessionCommandDto, MessageKindDto, MessageProjectionDto, RunModeDto, RunStatusDto,
    RunStreamFrameDto, SendUserTurnOutcomeDto, SubscribeRunCommandDto, WorkspaceRootDto,
    run_status_is_terminal,
};
use intention_proto::{IdempotencyKey, ProjectId, RunId, SessionId, WorkspaceId};
use intention_transport::LocalEndpoint;
use tempfile::TempDir;

/// One daemon-host fixture: isolated config/state/workspace, a fake provider,
/// one spawned daemon process, and its private endpoint.
///
/// Dropping the fixture kills the daemon, stops the provider, and removes the
/// daemon-owned Unix socket so a later run can bind the same logical endpoint.
struct E2eHost {
    config_home: TempDir,
    state_home: TempDir,
    workspace: TempDir,
    credential: String,
    provider: FakeProvider,
    daemon: Option<Child>,
    endpoint: LocalEndpoint,
}

impl E2eHost {
    /// Creates a fresh isolated fixture and spawns its first daemon process.
    fn new(workspace_file: Option<(&str, &str)>, tool_arguments: &str) -> Self {
        let config_home = TempDir::new().expect("config directory exists");
        let state_home = TempDir::new().expect("state directory exists");
        let workspace = TempDir::new().expect("workspace directory exists");
        if let Some((name, content)) = workspace_file {
            std::fs::write(workspace.path().join(name), content).expect("workspace fixture writes");
        }
        let credential = format!("fixture-credential-{}", std::process::id());
        let provider = FakeProvider::start(tool_arguments);
        let config_document = fixture_config_document(provider.port(), &credential);
        write_config_document(config_home.path(), &config_document);
        let endpoint = unique_endpoint("e2e");
        let daemon = spawn_daemon(&endpoint, config_home.path(), state_home.path(), None, &[]);
        Self {
            config_home,
            state_home,
            workspace,
            credential,
            provider,
            daemon: Some(daemon),
            endpoint,
        }
    }

    /// Kills the current daemon and starts a fresh process with identical
    /// environment, state directories, and endpoint.
    ///
    /// The kill is a hard kill: the daemon cannot run its listener Drop, so
    /// its Unix socket file survives. The transport reclaims the stale socket
    /// on the next bind (PR24-010); the fixture no longer deletes it by hand.
    fn restart_daemon(&mut self) {
        self.kill_daemon();
        self.daemon = Some(spawn_daemon(
            &self.endpoint,
            self.config_home.path(),
            self.state_home.path(),
            None,
            &[],
        ));
    }

    fn kill_daemon(&mut self) {
        let Some(mut child) = self.daemon.take() else {
            return;
        };
        let _ = child.kill();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                let _ = child.wait();
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for E2eHost {
    fn drop(&mut self) {
        self.kill_daemon();
        self.provider.stop();
        #[cfg(unix)]
        if let Some(path) = common::endpoint_socket_path(&self.endpoint) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Renders the fixture daemon configuration document for the fake provider.
fn fixture_config_document(port: u16, credential: &str) -> String {
    format!(
        "schema_version = 1\n\
         \n\
         [provider]\n\
         default_profile = \"main\"\n\
         \n\
         [providers.profiles.main]\n\
         kind = \"generic-chat-completion-api\"\n\
         model = \"fixture-model\"\n\
         endpoint = \"http://127.0.0.1:{port}/v1\"\n\
         credential = \"{credential}\"\n\
         display_name = \"Fixture\"\n\
         enabled = true\n\
         \n\
         [providers.profiles.main.execution]\n\
         attempt_timeout_seconds = 30\n\
         max_attempts = 2\n\
         \n\
         [providers.profiles.main.capabilities]\n\
         text_streaming = true\n\
         tool_exchange = true\n"
    )
}

/// A fake OpenAI-compatible provider serving two scripted SSE rounds.
///
/// The first request receives a tool-call round for the `read` tool, the second
/// (whose body carries the tool result) receives a text round, and any further
/// request receives an HTTP 500 and is counted as excess traffic.
struct FakeProvider {
    port: u16,
    requests: Arc<AtomicUsize>,
    excess: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FakeProvider {
    fn start(tool_arguments: &str) -> Self {
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
        let tool_response = sse_response(&format!(
            "data: {tool_body}\n\ndata: {usage_body}\n\ndata: [DONE]\n\n"
        ));
        let text_response = sse_response(&format!(
            "data: {text_body}\n\ndata: {usage_body}\n\ndata: [DONE]\n\n"
        ));
        let thread = thread::Builder::new()
            .name("client-e2e-provider".to_owned())
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

    const fn port(&self) -> u16 {
        self.port
    }

    fn request_count(&self) -> usize {
        self.requests.load(Ordering::Acquire)
    }

    fn excess_count(&self) -> usize {
        self.excess.load(Ordering::Acquire)
    }

    fn stop(&mut self) {
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

/// Waits for the spawned daemon to report `Ready` through the shared client.
///
/// `await_ready()` only connects and queries; it never launches the daemon.
/// Its own bounded wait is looped under the fixture deadline, so a daemon that
/// needs longer than one client budget still becomes ready in time.
async fn wait_until_ready(endpoint: &LocalEndpoint, deadline: Instant) -> IntentionClient {
    let client = IntentionClient::new(
        endpoint.clone(),
        Box::new(
            ProcessDaemonLauncher::new(env!("CARGO_BIN_EXE_intention-daemon"))
                .expect("daemon program is valid"),
        ),
    );
    while Instant::now() < deadline {
        if client.await_ready().await.is_ok() {
            return client;
        }
    }
    panic!("daemon becomes ready before the deadline");
}

/// One live run observation: the subscription, the correlated current-state
/// snapshot it carried, and every later frame the subscriber received.
struct LiveRunObservation {
    subscription: RunStreamSubscription,
    snapshot_status: Option<RunStatusDto>,
    snapshot_messages: Vec<MessageProjectionDto>,
    frames: Vec<RunStreamFrameDto>,
}

impl LiveRunObservation {
    /// Returns whether the correlated snapshot already reported a terminal run.
    fn snapshot_is_terminal(&self) -> bool {
        self.snapshot_status.is_some_and(run_status_is_terminal)
    }

    /// Returns whether any received status frame reported a committed status.
    fn received_status_frame(&self) -> bool {
        self.frames
            .iter()
            .any(|frame| matches!(frame, RunStreamFrameDto::Status(_)))
    }
}

/// Subscribes to one run's current state and drives the live stream until the
/// run reports a terminal status.
///
/// The correlated first reply is the current-state snapshot; every later
/// committed content or status change arrives as a live `run.frame` on the
/// same subscription. A re-subscribing client re-reads current state and
/// continues live, so both the snapshot and the frames are authoritative.
async fn observe_run_until_terminal(
    client: &RunStreamClient,
    session_id: SessionId,
    run_id: RunId,
    deadline: Instant,
) -> LiveRunObservation {
    let mut subscription = client
        .subscribe(SubscribeRunCommandDto::new(session_id, run_id))
        .await
        .expect("run subscription arrives");
    let snapshot_status = subscription.state().status();
    let snapshot_messages = subscription.state().messages().to_vec();
    let mut frames = Vec::new();
    loop {
        if subscription
            .state()
            .status()
            .is_some_and(run_status_is_terminal)
        {
            return LiveRunObservation {
                subscription,
                snapshot_status,
                snapshot_messages,
                frames,
            };
        }
        assert!(
            Instant::now() < deadline,
            "the run reaches a terminal status before the deadline"
        );
        match tokio::time::timeout(Duration::from_secs(1), subscription.receive()).await {
            Ok(Ok(Some(frame))) => frames.push(frame),
            Ok(Ok(None)) => {
                panic!("the daemon closed the run stream before the run terminalized")
            }
            Ok(Err(error)) => panic!("run stream frame error: {}", error.code()),
            Err(_) => {}
        }
    }
}

/// Returns whether one transcript slice holds the committed assistant step
/// whose text the fixture provider streamed.
fn has_assistant_step(messages: &[MessageProjectionDto]) -> bool {
    messages
        .iter()
        .any(|message| message.kind() == MessageKindDto::Assistant && message.text() == "done")
}

#[tokio::test]
async fn real_daemon_tool_loop_executes_read_and_replays_after_restart() {
    let mut host = E2eHost::new(
        Some(("hello.txt", "hello from e2e")),
        r#"{"path":"hello.txt"}"#,
    );
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20)).await;
    let workspace_root = host.workspace.path().to_string_lossy().into_owned();

    let session_id = SessionId::new();
    client
        .create_session(CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse(workspace_root).expect("workspace root is absolute"),
            RunModeDto::Build,
        ))
        .await
        .expect("the daemon accepts session creation");

    let outcome = client
        .send_user_turn(
            session_id,
            IdempotencyKey::new(),
            "Read hello.txt".to_owned(),
        )
        .await
        .expect("user turn is accepted");
    let SendUserTurnOutcomeDto::Started { run_id, .. } = outcome else {
        panic!("first turn starts a run, got: {outcome:?}")
    };

    let stream_client = RunStreamClient::new(host.endpoint.clone());
    let live_deadline = Instant::now() + Duration::from_secs(30);
    // Subscribe immediately after acceptance: the subscription snapshot is the
    // current run state, and every later commit arrives as a live frame.
    let observation =
        observe_run_until_terminal(&stream_client, session_id, run_id, live_deadline).await;
    let state = observation.subscription.state();
    assert_eq!(
        state.status(),
        Some(RunStatusDto::Completed),
        "the real daemon completes the tool round"
    );
    assert_eq!(
        state
            .run()
            .expect("the subscription carries the run scope")
            .run_id(),
        run_id
    );
    // A non-terminal snapshot can only reach the terminal status through a
    // live status frame; the committed completion is published, never polled.
    if !observation.snapshot_is_terminal() {
        assert!(
            observation.received_status_frame(),
            "a live status frame carries the committed completion to the subscriber"
        );
    }
    // The committed assistant step is published live or carried by the
    // snapshot, so the subscriber always observes it exactly once.
    if !has_assistant_step(&observation.snapshot_messages) {
        assert!(
            observation.frames.iter().any(|frame| matches!(
                frame,
                RunStreamFrameDto::Content(message)
                    if message.kind() == MessageKindDto::Assistant && message.text() == "done"
            )),
            "the committed assistant content is published as a live content frame"
        );
    }
    // The committed tool round is current state for the subscriber: the tool
    // call and its one result row are carried by the snapshot or published as
    // live content frames after their own commits.
    let subscriber_messages = state.messages();
    assert!(
        subscriber_messages
            .iter()
            .any(|message| message.kind() == MessageKindDto::ToolCall),
        "the subscriber's transcript holds the committed tool call"
    );
    assert!(
        subscriber_messages
            .iter()
            .any(|message| message.kind() == MessageKindDto::ToolResult),
        "the subscriber's transcript holds the committed tool result"
    );
    if !observation
        .snapshot_messages
        .iter()
        .any(|message| message.kind() == MessageKindDto::ToolResult)
    {
        assert!(
            observation.frames.iter().any(|frame| matches!(
                frame,
                RunStreamFrameDto::Content(message)
                    if message.kind() == MessageKindDto::ToolResult
            )),
            "the committed tool result is published as a live content frame"
        );
    }
    assert!(
        has_assistant_step(state.messages()),
        "the subscriber's current transcript holds the committed assistant step"
    );

    // The session snapshot is the current-state read of the committed
    // transcript: the accepted turn, the tool call, its one result row, and the
    // assistant step, in durable insertion order.
    let session_snapshot = client
        .session_snapshot(session_id)
        .await
        .expect("session snapshot reads");
    let messages = session_snapshot.messages();
    assert_eq!(
        messages
            .iter()
            .map(|message| message.kind())
            .collect::<Vec<_>>(),
        vec![
            MessageKindDto::User,
            MessageKindDto::ToolCall,
            MessageKindDto::ToolResult,
            MessageKindDto::Assistant,
        ],
        "the committed transcript records the turn, the read call, its result, and the assistant step"
    );
    assert_eq!(messages[0].text(), "Read hello.txt");
    assert_eq!(messages[1].tool_id(), Some("read"));
    assert_eq!(
        messages[1].text(),
        r#"{"path":"hello.txt"}"#,
        "the durable tool call keeps the relative workspace path"
    );
    let call_id = messages[1]
        .tool_call_id()
        .expect("the tool call row carries its identity");
    assert_eq!(messages[2].tool_id(), Some("read"));
    assert_eq!(messages[2].tool_call_id(), Some(call_id));
    assert_eq!(
        messages[2].text(),
        "hello from e2e",
        "the tool result row carries the exact file content"
    );
    assert_eq!(messages[3].text(), "done");
    assert_eq!(
        host.provider.request_count(),
        2,
        "the provider serves exactly the tool round and its follow-up round"
    );
    assert_eq!(
        host.provider.excess_count(),
        0,
        "no further provider request follows completion"
    );

    // Public payloads never disclose the credential; the durable transcript
    // never discloses the absolute workspace path.
    let transcript_json = serde_json::to_string(messages).expect("transcript serializes");
    let session_json =
        serde_json::to_string(&session_snapshot).expect("session snapshot serializes");
    assert!(
        !transcript_json.contains(&host.credential),
        "the durable transcript never discloses the provider credential"
    );
    assert!(
        !transcript_json.contains(&host.workspace.path().to_string_lossy().into_owned()),
        "the durable transcript never discloses the absolute workspace path"
    );
    assert!(
        !session_json.contains(&host.credential),
        "the session snapshot never discloses the provider credential"
    );

    // Restart the daemon against the identical environment and state, then
    // prove the completed run is replayed as current state without any
    // provider re-execution.
    host.restart_daemon();
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20)).await;
    let replayed = observe_run_until_terminal(
        &stream_client,
        session_id,
        run_id,
        Instant::now() + Duration::from_secs(15),
    )
    .await;
    assert_eq!(
        replayed.snapshot_status,
        Some(RunStatusDto::Completed),
        "the restarted daemon replays the completed run as its current state"
    );
    let replayed_messages = replayed.subscription.state().messages();
    assert_eq!(
        replayed_messages.len(),
        4,
        "the replayed run snapshot carries the complete committed transcript"
    );
    assert_eq!(
        replayed_messages[2].text(),
        "hello from e2e",
        "the durable tool result replays unchanged"
    );
    let replayed_session = client
        .session_snapshot(session_id)
        .await
        .expect("replayed session snapshot reads");
    assert_eq!(
        replayed_session
            .messages()
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        session_snapshot
            .messages()
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        "the durable transcript replays unchanged after the daemon restart"
    );

    // Allow any late provider traffic to land before asserting the tool was
    // never re-executed after the restart.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        host.provider.request_count(),
        2,
        "the restarted daemon replays the durable run without re-executing the tool"
    );
    assert_eq!(host.provider.excess_count(), 0);
}

#[tokio::test]
async fn real_daemon_tool_loop_denies_without_provider_retry_on_tool_failure() {
    let host = E2eHost::new(None, r#"{"path":"missing.txt"}"#);
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20)).await;

    let session_id = SessionId::new();
    client
        .create_session(CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse(host.workspace.path().to_string_lossy().into_owned())
                .expect("workspace root is absolute"),
            RunModeDto::Build,
        ))
        .await
        .expect("the daemon accepts session creation");

    let outcome = client
        .send_user_turn(
            session_id,
            IdempotencyKey::new(),
            "Read missing.txt".to_owned(),
        )
        .await
        .expect("user turn is accepted");
    let SendUserTurnOutcomeDto::Started { run_id, .. } = outcome else {
        panic!("first turn starts a run, got: {outcome:?}")
    };

    let stream_client = RunStreamClient::new(host.endpoint.clone());
    let deadline = Instant::now() + Duration::from_secs(30);
    let observation =
        observe_run_until_terminal(&stream_client, session_id, run_id, deadline).await;
    assert_eq!(
        observation.subscription.state().status(),
        Some(RunStatusDto::Failed),
        "the real daemon terminalizes the missing-file tool round as Failed"
    );
    if !observation.snapshot_is_terminal() {
        assert!(
            observation.received_status_frame(),
            "a live status frame carries the committed failure to the subscriber"
        );
    }
    assert_eq!(
        host.provider.request_count(),
        1,
        "the typed tool failure never retries the provider"
    );
    assert_eq!(host.provider.excess_count(), 0);

    // The failed run's transcript carries the model's call row and the typed
    // failure result row committed for that exact invocation.
    let session_snapshot = client
        .session_snapshot(session_id)
        .await
        .expect("session snapshot reads");
    let messages = session_snapshot.messages();
    assert_eq!(
        messages
            .iter()
            .map(|message| message.kind())
            .collect::<Vec<_>>(),
        vec![
            MessageKindDto::User,
            MessageKindDto::ToolCall,
            MessageKindDto::ToolResult,
        ],
        "the failed run records the turn, the denied call, and its typed failure result"
    );
    assert_eq!(messages[0].text(), "Read missing.txt");
    assert_eq!(messages[1].tool_id(), Some("read"));
    assert_eq!(
        messages[1].text(),
        r#"{"path":"missing.txt"}"#,
        "the denied tool call keeps the relative workspace path"
    );
    let call_id = messages[1]
        .tool_call_id()
        .expect("the tool call row carries its identity");
    assert_eq!(messages[2].tool_id(), Some("read"));
    assert_eq!(messages[2].tool_call_id(), Some(call_id));
    assert_eq!(
        messages[2].text(),
        "tool_read_failed",
        "the missing-file tool result is durable typed-failure evidence"
    );
}
