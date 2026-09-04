//! Facade-level daemon-host end-to-end tests over real IPC.
//!
//! These tests spawn the real `intention-daemon` binary over real local
//! transport, drive it with the real client transport, and execute a real
//! `read` tool through the production model-tool loop against a fake
//! OpenAI-compatible provider. They prove durable `ToolResultRecorded` facts,
//! restart the daemon, and prove the same durable run replays without
//! re-executing the tool.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Facade end-to-end fixtures use assertion conveniences for precise diagnostics."
)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use intention_client::{IntentionClient, ProcessDaemonLauncher, RunStreamClient};
use intention_domain::{
    CreateSessionCommandDto, ModelRunFactDto, ModelRunFactInputDto, RunModeDto, RunSnapshotDto,
    RunStatusDto, SendUserTurnCommandDto, ToolResultOutcomeDto, WorkspaceRootDto,
};
use intention_protocol::{
    DaemonReadinessDto, ProtocolAcceptedResultDto, ProtocolCapabilityDto, ProtocolCommandDto,
    ProtocolCommandResultDto, ProtocolDaemonFrameDto, ProtocolHelloDto, ProtocolMessageDto,
    ProtocolRequestEnvelopeDto, ProtocolRequestPayloadDto, ProtocolResponsePayloadDto,
    RunStreamFrameDto, RunSubscriptionRequestEnvelopeDto, RunSubscriptionResponseDto,
    SendUserTurnOutcomeDto, SubscribeRunCommandDto,
};
use intention_transport::{
    AsyncLocalClientConnection, LocalConnection, LocalEndpoint, local_protocol_version,
    negotiate_client,
};
use intention_types::{
    CorrelationIdDto, DtoResult, ErrorDto, ProjectId, RunId, SessionId, TurnId, WorkspaceId,
};
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
        write_config(config_home.path(), provider.port(), &credential);
        let endpoint = unique_endpoint();
        let daemon = spawn_daemon(&endpoint, config_home.path(), state_home.path());
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
        if let Some(path) = endpoint_socket_path(&self.endpoint) {
            let _ = std::fs::remove_file(path);
        }
    }
}

static NEXT_ENDPOINT: AtomicUsize = AtomicUsize::new(0);

/// Builds a unique safe endpoint instance id for this test process.
fn unique_endpoint() -> LocalEndpoint {
    let sequence = NEXT_ENDPOINT.fetch_add(1, Ordering::Relaxed);
    LocalEndpoint::from_instance_id(format!("e2e-{}-{}", std::process::id(), sequence))
        .expect("fixture endpoint is valid")
}

/// Spawns the real daemon binary with only per-process environment overrides.
fn spawn_daemon(endpoint: &LocalEndpoint, config_home: &Path, state_home: &Path) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_intention-daemon"));
    command.arg(endpoint.instance_id());
    #[cfg(target_os = "linux")]
    {
        command.env("XDG_CONFIG_HOME", config_home);
        command.env("XDG_STATE_HOME", state_home);
    }
    #[cfg(target_os = "macos")]
    {
        // Both the configuration and the state directory derive from HOME.
        command.env("HOME", config_home);
    }
    #[cfg(windows)]
    {
        command.env("APPDATA", config_home);
        command.env("LOCALAPPDATA", state_home);
    }
    command.spawn().expect("daemon binary spawns")
}

/// Returns the platform config path the daemon resolves from its environment.
fn config_path(config_home: &Path) -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        config_home.join("intention-relay").join("config.toml")
    }
    #[cfg(target_os = "macos")]
    {
        config_home
            .join("Library/Application Support/intention-relay")
            .join("config.toml")
    }
    #[cfg(windows)]
    {
        config_home.join("intention-relay").join("config.toml")
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        config_home.join("intention-relay").join("config.toml")
    }
}

/// Writes the daemon configuration file with owner-only permissions on Unix.
fn write_config(config_home: &Path, port: u16, credential: &str) {
    let config_text = format!(
        "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"fixture-model\"\nendpoint = \"http://127.0.0.1:{port}/v1\"\ncredential = \"{credential}\"\n"
    );
    let config_path = config_path(config_home);
    let parent = config_path.parent().expect("config path has a parent");
    std::fs::create_dir_all(parent).expect("config directory is created");
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).write(true).truncate(true).mode(0o600);
        let mut file = options.open(&config_path).expect("config file opens");
        file.write_all(config_text.as_bytes())
            .expect("config file writes");
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&config_path, config_text).expect("config file writes");
    }
}

/// Replicates the daemon transport's platform endpoint path resolution so the
/// fixture can remove exactly the socket file its own daemon created.
#[cfg(unix)]
fn endpoint_socket_path(endpoint: &LocalEndpoint) -> Option<PathBuf> {
    let base = {
        #[cfg(target_os = "linux")]
        {
            std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .filter(|candidate| candidate.is_absolute())
                .or_else(|| {
                    std::env::var_os("XDG_CONFIG_HOME")
                        .map(PathBuf::from)
                        .filter(|candidate| candidate.is_absolute())
                        .map(|candidate| candidate.join("intention-relay"))
                })
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .filter(|candidate| candidate.is_absolute())
                        .map(|candidate| candidate.join(".config/intention-relay"))
                })
        }
        #[cfg(target_os = "macos")]
        {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|candidate| candidate.is_absolute())
                .map(|candidate| candidate.join("Library/Application Support/intention-relay"))
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            None
        }
    }?;
    Some(base.join(format!("{}.sock", endpoint.instance_id())))
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
            "id": "chatcmpl-facade-e2e-1",
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
            "id": "chatcmpl-facade-e2e-1",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "fixture-model",
            "choices": [],
            "usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5},
        }))
        .expect("usage chunk serializes");
        let text_body = serde_json::to_string(&serde_json::json!({
            "id": "chatcmpl-facade-e2e-2",
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
            .name("facade-e2e-provider".to_owned())
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
    // attach before the durable facts for that round are committed.
    thread::sleep(Duration::from_millis(500));
    let request_number = requests.fetch_add(1, Ordering::AcqRel) + 1;
    let body_text = String::from_utf8_lossy(&body);
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

/// The exact capability list the shared client requires from the daemon.
///
/// The list itself is private to `intention-client`, but its values are public
/// protocol capabilities and the daemon's negotiation only verifies protocol
/// version compatibility, so the fixture reconstructs the same hello with
/// public APIs only.
fn command_hello() -> ProtocolHelloDto {
    ProtocolHelloDto::new(
        local_protocol_version(),
        vec![
            ProtocolCapabilityDto::SessionSubscriptions,
            ProtocolCapabilityDto::CorrelatedRequests,
            ProtocolCapabilityDto::DaemonHealth,
        ],
        "facade-e2e",
    )
    .expect("fixture command hello is valid")
}

fn stream_hello() -> ProtocolHelloDto {
    ProtocolHelloDto::new(
        local_protocol_version(),
        vec![ProtocolCapabilityDto::RunStreamSubscriptions],
        "facade-e2e",
    )
    .expect("fixture stream hello is valid")
}

/// Sends one typed protocol command over a fresh negotiated connection and
/// verifies the correlated response, replicating the client's private request
/// path with public transport and protocol APIs only.
fn send_command(
    endpoint: &LocalEndpoint,
    payload: ProtocolRequestPayloadDto,
) -> DtoResult<ProtocolCommandResultDto> {
    let mut connection = LocalConnection::connect(endpoint)?;
    let remote = negotiate_client(&mut connection, command_hello())?;
    let correlation_id = CorrelationIdDto::new();
    connection.send_request(&ProtocolRequestEnvelopeDto::new(
        local_protocol_version(),
        correlation_id,
        ProtocolMessageDto::new(intention_protocol::CURRENT_DTO_SCHEMA_VERSION, payload),
    ))?;
    let response = connection.receive_response()?;
    if response.correlation_id() != correlation_id
        || response.protocol_version() != remote.version()
    {
        return Err(invalid_response());
    }
    match response.message().payload() {
        ProtocolResponsePayloadDto::CommandResult(result) => Ok(result.clone()),
        _ => Err(invalid_response()),
    }
}

fn invalid_response() -> ErrorDto {
    ErrorDto::validation(
        "invalid_local_protocol_response",
        "the local daemon returned an unexpected protocol response",
    )
}

/// Polls the daemon health projection until it reports `Ready`.
///
/// `health()` only negotiates and queries; it never launches the daemon.
fn wait_until_ready(endpoint: &LocalEndpoint, deadline: Instant) -> IntentionClient {
    let client = IntentionClient::new(
        endpoint.clone(),
        "facade-e2e",
        Box::new(
            ProcessDaemonLauncher::new(env!("CARGO_BIN_EXE_intention-daemon"))
                .expect("daemon program is valid"),
        ),
    )
    .expect("facade e2e client is valid");
    while Instant::now() < deadline {
        match client.health() {
            Ok(health) if health.readiness() == DaemonReadinessDto::Ready => return client,
            Ok(_) => {}
            Err(_) => {}
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("daemon becomes ready before the deadline");
}

/// Drives the real `RunStreamClient` until the run reaches a terminal snapshot.
async fn observe_terminal_snapshot(
    client: &RunStreamClient,
    session_id: SessionId,
    run_id: RunId,
    deadline: Instant,
) -> RunSnapshotDto {
    let mut subscription = client
        .subscribe(SubscribeRunCommandDto::new(
            intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
            session_id,
            run_id,
            None,
        ))
        .await
        .expect("run subscription arrives");
    loop {
        if let Some(snapshot) = subscription.reducer().snapshot()
            && snapshot.run_projection().status().is_terminal()
        {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "run reaches a terminal snapshot before the deadline"
        );
        match tokio::time::timeout(Duration::from_secs(1), subscription.receive()).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => panic!("run stream frame error: {}", error.code()),
            Err(_) => {}
        }
    }
}

/// Subscribes to one run stream and collects every delivered durable fact plus
/// the terminal snapshot. The daemon only replays the snapshot and tail on
/// subscribe; facts are delivered as live batches while the run commits them.
async fn collect_run_facts(
    endpoint: &LocalEndpoint,
    session_id: SessionId,
    run_id: RunId,
    deadline: Instant,
) -> (Vec<ModelRunFactDto>, RunSnapshotDto) {
    let connection = AsyncLocalClientConnection::connect(endpoint)
        .await
        .expect("run stream connects");
    let (_remote, mut requests, mut frames) = connection
        .negotiate_daemon_frames(stream_hello())
        .await
        .expect("run stream negotiates");
    let correlation_id = CorrelationIdDto::new();
    requests
        .send_run_subscription(&RunSubscriptionRequestEnvelopeDto::new(
            local_protocol_version(),
            correlation_id,
            ProtocolMessageDto::new(
                intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
                SubscribeRunCommandDto::new(
                    intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
                    session_id,
                    run_id,
                    None,
                ),
            ),
        ))
        .await
        .expect("run subscription request sends");
    let mut facts = Vec::new();
    loop {
        assert!(
            Instant::now() < deadline,
            "run facts arrive before the deadline"
        );
        let frame = tokio::time::timeout(Duration::from_secs(1), frames.receive())
            .await
            .expect("run stream frame within the deadline")
            .expect("run stream frame is valid");
        match frame {
            ProtocolDaemonFrameDto::Response(response) => {
                assert_eq!(response.correlation_id(), correlation_id);
                match response.message().payload() {
                    ProtocolResponsePayloadDto::RunSubscription(
                        RunSubscriptionResponseDto::Replay(replay),
                    ) => {
                        facts.extend(replay.tail().facts().iter().cloned());
                        let snapshot = replay.snapshot().clone();
                        if snapshot.run_projection().status().is_terminal() {
                            return (facts, snapshot);
                        }
                    }
                    _ => panic!("run subscription reply must be a replay"),
                }
            }
            ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::LiveBatch(batch)) => {
                facts.extend(batch.facts().iter().cloned());
            }
            ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::Snapshot(frame)) => {
                let snapshot = frame.snapshot().clone();
                if snapshot.run_projection().status().is_terminal() {
                    return (facts, snapshot);
                }
            }
            ProtocolDaemonFrameDto::RunStream(RunStreamFrameDto::Resync(resync)) => {
                panic!("unexpected run resync: {:?}", resync.reason());
            }
        }
    }
}

/// Asserts the delivered durable facts are a non-empty contiguous cursor range
/// ending in a terminal fact (`Finished` or `Failed`).
///
/// The daemon replays an empty tail on subscribe and broadcasts live batches
/// only for facts committed after registration, so a subscriber can miss a
/// prefix of the run's facts but never a gap inside the delivered suffix.
fn assert_contiguous_facts(facts: &[ModelRunFactDto]) {
    assert!(!facts.is_empty(), "durable facts are delivered");
    assert!(
        facts
            .windows(2)
            .all(|pair| pair[0].cursor().value() + 1 == pair[1].cursor().value()),
        "durable facts are contiguous in cursor order"
    );
    assert!(
        matches!(
            facts.last().map(ModelRunFactDto::input),
            Some(ModelRunFactInputDto::Finished { .. }) | Some(ModelRunFactInputDto::Failed { .. })
        ),
        "the final durable fact closes the run"
    );
}

#[tokio::test]
async fn real_daemon_tool_loop_executes_read_and_replays_after_restart() {
    let mut host = E2eHost::new(
        Some(("hello.txt", "hello from e2e")),
        r#"{"path":"hello.txt"}"#,
    );
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20));
    let workspace_root = host.workspace.path().to_string_lossy().into_owned();

    let session_id = SessionId::new();
    let created = send_command(
        &host.endpoint,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                WorkspaceRootDto::parse(workspace_root).expect("workspace root is absolute"),
                RunModeDto::Build,
            ),
        )),
    )
    .expect("session creation is accepted");
    assert!(
        matches!(created, ProtocolCommandResultDto::Accepted(_)),
        "the daemon accepts session creation"
    );

    let result = send_command(
        &host.endpoint,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, TurnId::new(), "Read hello.txt")
                .expect("turn is valid"),
        )),
    )
    .expect("user turn is accepted");
    let ProtocolCommandResultDto::Accepted(accepted) = result else {
        panic!("user turn starts a run, got: {result:?}")
    };
    let ProtocolAcceptedResultDto::SendUserTurn(turn) = accepted.result() else {
        panic!("user turn result starts a run, got: {accepted:?}")
    };
    let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
        panic!("first turn starts a run, got: {turn:?}")
    };

    let stream_client =
        RunStreamClient::new(host.endpoint.clone(), "facade-e2e").expect("stream client is valid");
    let live_deadline = Instant::now() + Duration::from_secs(30);
    // Subscribe for facts first: the daemon broadcasts live batches only while
    // the run commits, and the run-stream replay tail is empty by design.
    let (facts, snapshot) =
        collect_run_facts(&host.endpoint, session_id, run_id, live_deadline).await;
    let terminal_snapshot =
        observe_terminal_snapshot(&stream_client, session_id, run_id, live_deadline).await;
    assert_eq!(
        terminal_snapshot.run_projection().status(),
        RunStatusDto::Completed,
        "the real daemon completes the tool round: {:?}",
        terminal_snapshot.projection().failure()
    );
    assert_contiguous_facts(&facts);
    let tool_call = facts
        .iter()
        .find_map(|fact| match fact.input() {
            ModelRunFactInputDto::ToolCallRecorded { call } if call.name() == "read" => {
                Some(call.clone())
            }
            _ => None,
        })
        .expect("the read tool call is durable");
    assert_eq!(
        tool_call.arguments_json(),
        r#"{"path":"hello.txt"}"#,
        "the durable tool call keeps the relative workspace path"
    );
    let tool_result = facts
        .iter()
        .find_map(|fact| match fact.input() {
            ModelRunFactInputDto::ToolResultRecorded { call_id, outcome }
                if *call_id == tool_call.call_id() =>
            {
                Some(outcome)
            }
            _ => None,
        })
        .expect("the read tool result is durable");
    assert!(
        matches!(
            tool_result,
            ToolResultOutcomeDto::Succeeded { content } if content == "hello from e2e"
        ),
        "the durable tool result carries the exact file content"
    );
    assert_eq!(snapshot.run_projection().status(), RunStatusDto::Completed);
    assert_eq!(
        facts.last().expect("facts exist").cursor(),
        snapshot.cursor(),
        "the terminal fact cursor equals the authoritative snapshot cursor"
    );
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

    // Public payloads never disclose the credential; durable run facts never
    // disclose the absolute workspace path.
    let session_json = serde_json::to_string(
        &client
            .session_snapshot(session_id)
            .expect("session snapshot reads"),
    )
    .expect("session snapshot serializes");
    let facts_json = serde_json::to_string(&facts).expect("durable facts serialize");
    assert!(
        !facts_json.contains(&host.credential),
        "durable facts never disclose the provider credential"
    );
    assert!(
        !facts_json.contains(&host.workspace.path().to_string_lossy().into_owned()),
        "durable facts never disclose the absolute workspace path"
    );
    assert!(
        !session_json.contains(&host.credential),
        "the session snapshot never discloses the provider credential"
    );
    let session_sequence = client
        .session_snapshot(session_id)
        .expect("session snapshot reads")
        .at_sequence();
    let pre_restart_cursor = snapshot.cursor();

    // Restart the daemon against the identical environment and state, then
    // prove the completed run replays without any provider re-execution.
    host.restart_daemon();
    let client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20));
    let mut subscription = stream_client
        .subscribe(SubscribeRunCommandDto::new(
            intention_protocol::CURRENT_DTO_SCHEMA_VERSION,
            session_id,
            run_id,
            None,
        ))
        .await
        .expect("restart replay arrives");
    let replay_snapshot = subscription
        .reducer()
        .snapshot()
        .expect("restart replay is authoritative");
    assert_eq!(
        replay_snapshot.run_projection().status(),
        RunStatusDto::Completed,
        "the restarted daemon replays the completed run"
    );
    assert_eq!(
        replay_snapshot.cursor(),
        pre_restart_cursor,
        "the durable run cursor replays unchanged"
    );
    subscription
        .request_replay()
        .await
        .expect("repeat replay arrives");
    assert_eq!(
        subscription
            .reducer()
            .snapshot()
            .expect("repeat replay is authoritative")
            .run_projection()
            .status(),
        RunStatusDto::Completed,
        "the same connection accepts a repeated replay"
    );
    let (replayed_facts, replayed_snapshot) = collect_run_facts(
        &host.endpoint,
        session_id,
        run_id,
        Instant::now() + Duration::from_secs(15),
    )
    .await;
    assert_eq!(
        replayed_snapshot.run_projection().status(),
        RunStatusDto::Completed
    );
    assert_eq!(
        replayed_snapshot.cursor(),
        pre_restart_cursor,
        "the replayed snapshot preserves the exact durable cursor"
    );
    assert!(
        replayed_facts.is_empty(),
        "the daemon run-stream replay tail is empty by design; live batches carry facts"
    );
    let replayed_session_sequence = client
        .session_snapshot(session_id)
        .expect("replayed session snapshot reads")
        .at_sequence();
    assert_eq!(
        replayed_session_sequence, session_sequence,
        "the durable session event sequence replays unchanged"
    );

    // Allow any late provider traffic to land before asserting the tool was
    // never re-executed after the restart.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        host.provider.request_count(),
        2,
        "the restarted daemon replays durable facts without re-executing the tool"
    );
    assert_eq!(host.provider.excess_count(), 0);
}

#[tokio::test]
async fn real_daemon_tool_loop_denies_without_provider_retry_on_tool_failure() {
    let host = E2eHost::new(None, r#"{"path":"missing.txt"}"#);
    let _client = wait_until_ready(&host.endpoint, Instant::now() + Duration::from_secs(20));

    let session_id = SessionId::new();
    let created = send_command(
        &host.endpoint,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                WorkspaceRootDto::parse(host.workspace.path().to_string_lossy().into_owned())
                    .expect("workspace root is absolute"),
                RunModeDto::Build,
            ),
        )),
    )
    .expect("session creation is accepted");
    assert!(matches!(created, ProtocolCommandResultDto::Accepted(_)));

    let result = send_command(
        &host.endpoint,
        ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, TurnId::new(), "Read missing.txt")
                .expect("turn is valid"),
        )),
    )
    .expect("user turn is accepted");
    let ProtocolCommandResultDto::Accepted(accepted) = result else {
        panic!("user turn starts a run, got: {result:?}")
    };
    let ProtocolAcceptedResultDto::SendUserTurn(turn) = accepted.result() else {
        panic!("user turn result starts a run, got: {accepted:?}")
    };
    let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
        panic!("first turn starts a run, got: {turn:?}")
    };

    let stream_client =
        RunStreamClient::new(host.endpoint.clone(), "facade-e2e").expect("stream client is valid");
    let deadline = Instant::now() + Duration::from_secs(30);
    let (facts, snapshot) = collect_run_facts(&host.endpoint, session_id, run_id, deadline).await;
    let terminal_snapshot =
        observe_terminal_snapshot(&stream_client, session_id, run_id, deadline).await;
    assert_eq!(
        terminal_snapshot.run_projection().status(),
        RunStatusDto::Failed,
        "the real daemon terminalizes the missing-file tool round as Failed: {:?}",
        terminal_snapshot.projection().failure()
    );
    assert_contiguous_facts(&facts);
    assert!(
        facts.iter().any(|fact| matches!(
            fact.input(),
            ModelRunFactInputDto::ToolCallRecorded { call }
                if call.name() == "read"
        )),
        "the denied read tool call is durable"
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact.input(),
            ModelRunFactInputDto::ToolResultRecorded {
                outcome: ToolResultOutcomeDto::Failed { failure },
                ..
            } if failure.code() == "workspace_path_unavailable"
        )),
        "the missing-file tool result is a durable typed failure"
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact.input(),
            ModelRunFactInputDto::Failed { failure }
                if failure.code() == "workspace_path_unavailable"
        )),
        "the run terminalizes with the durable tool failure"
    );
    assert_eq!(snapshot.run_projection().status(), RunStatusDto::Failed);
    assert_eq!(
        host.provider.request_count(),
        1,
        "the typed tool failure never retries the provider"
    );
    assert_eq!(host.provider.excess_count(), 0);
}
